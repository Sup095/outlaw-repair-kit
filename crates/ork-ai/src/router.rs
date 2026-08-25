//! Choosing which model handles this run.
//!
//! The order is remote, then local, then cloud, and each step is tried only if
//! the one before it is unavailable or the user has not overridden the choice.
//! The reasoning behind that order is about where the data goes: a machine the
//! user owns on their own network first, this machine second, and a third
//! party only when they have explicitly asked for it.
//!
//! Every decision is recorded. A tool that silently picks a different model
//! than the user expects -- and silently sends their diagnostics somewhere
//! else -- is worse than one that fails loudly, so the router returns the full
//! list of what it tried and why each option was or was not used.

use std::sync::Arc;
use std::time::Duration;

use ork_core::config::{AiConfig, RoutingMode};
use ork_core::platform::GpuInfo;

use crate::client::{AnthropicClient, ModelClient, OpenAiCompatibleClient, list_models};
use crate::secrets::{self, SecretKind};

/// Which of the three tiers a route uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    /// A model on another machine the user owns.
    Remote,
    /// A model on this machine.
    Local,
    /// A hosted model, reached over the internet.
    Cloud,
}

impl ModelTier {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelTier::Remote => "remote",
            ModelTier::Local => "local",
            ModelTier::Cloud => "cloud",
        }
    }

    /// Whether choosing this tier sends data off the user's own hardware.
    pub fn leaves_your_network(self) -> bool {
        matches!(self, ModelTier::Cloud)
    }
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happened when the router considered one tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// This is the tier that will be used.
    Selected { detail: String },
    /// Turned off in settings.
    Disabled,
    /// On, but nothing has been configured to point at.
    NotConfigured { detail: String },
    /// Configured, but nothing answered.
    Unreachable { detail: String },
    /// Configured, but no credential is available.
    MissingCredential { detail: String },
    /// Ruled out because the user pinned a different tier.
    NotSelectedByUser,
}

impl AttemptOutcome {
    pub fn is_selected(&self) -> bool {
        matches!(self, AttemptOutcome::Selected { .. })
    }

    /// One line explaining this outcome to a person.
    pub fn explain(&self) -> String {
        match self {
            AttemptOutcome::Selected { detail } => format!("selected -- {detail}"),
            AttemptOutcome::Disabled => "turned off in settings".to_string(),
            AttemptOutcome::NotConfigured { detail } => format!("not set up -- {detail}"),
            AttemptOutcome::Unreachable { detail } => format!("did not answer -- {detail}"),
            AttemptOutcome::MissingCredential { detail } => format!("no credential -- {detail}"),
            AttemptOutcome::NotSelectedByUser => {
                "skipped because a different tier is pinned in settings".to_string()
            }
        }
    }
}

/// One tier the router looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteAttempt {
    pub tier: ModelTier,
    pub outcome: AttemptOutcome,
}

/// The outcome of routing: what was chosen, and everything that was tried.
pub struct Routing {
    /// The client to use, if any tier worked out.
    pub client: Option<Arc<dyn ModelClient>>,
    pub tier: Option<ModelTier>,
    /// Every tier considered, in the order they were considered.
    pub attempts: Vec<RouteAttempt>,
}

impl Routing {
    /// Whether any model is available at all.
    pub fn is_available(&self) -> bool {
        self.client.is_some()
    }

    /// A one-line summary for the user.
    pub fn summary(&self) -> String {
        match (&self.tier, &self.client) {
            (Some(tier), Some(client)) => format!("{tier}: {}", client.describe()),
            _ => "no model available -- deterministic checks and runbooks only".to_string(),
        }
    }
}

/// How large a local model this machine can comfortably hold.
///
/// This is advice, not a decision. The tool does not load models -- it asks
/// whatever server is running to use one -- so the honest thing is to tell the
/// user what their hardware can take and let them choose, rather than to
/// pretend to a control it does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VramAdvice {
    pub vram_bytes: Option<u64>,
    pub recommendation: String,
}

const GIB: u64 = 1024 * 1024 * 1024;

pub fn advise_for_vram(gpus: &[GpuInfo]) -> VramAdvice {
    let vram_bytes = gpus.iter().filter_map(|gpu| gpu.vram_total_bytes).max();

    let recommendation = match vram_bytes {
        None => "Video memory could not be determined, so no size can be recommended. \
                 Install the graphics vendor's tools, or choose a model yourself."
            .to_string(),
        Some(bytes) if bytes < 4 * GIB => {
            "Under 4 GB of video memory. A local model will be slow and heavily limited; \
             consider using a model on another machine instead."
                .to_string()
        }
        Some(bytes) if bytes < 8 * GIB => {
            "Around 6 GB of video memory. A 7-8B model at 4-bit quantisation is the \
             practical ceiling. Larger analysis is better sent to another machine."
                .to_string()
        }
        Some(bytes) if bytes < 16 * GIB => {
            "8-12 GB of video memory. A 13-14B model at 4-bit quantisation fits \
             comfortably."
                .to_string()
        }
        Some(bytes) if bytes < 24 * GIB => {
            "16-20 GB of video memory. A 30B-class model at 4-bit quantisation fits.".to_string()
        }
        Some(_) => "24 GB or more of video memory. A 70B-class model at 4-bit quantisation \
                    fits, and this machine is a good candidate for serving other machines \
                    on the network."
            .to_string(),
    };

    VramAdvice {
        vram_bytes,
        recommendation,
    }
}

/// Picks a model according to configuration and what is actually reachable.
pub struct ModelRouter {
    config: AiConfig,
}

impl ModelRouter {
    pub fn new(config: AiConfig) -> Self {
        Self { config }
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.config.reachability_timeout_ms.max(100))
    }

    /// Whether a tier should be considered at all, given the user's pinning.
    fn wanted(&self, tier: ModelTier) -> bool {
        match self.config.mode {
            RoutingMode::Auto => true,
            RoutingMode::Remote => tier == ModelTier::Remote,
            RoutingMode::Local => tier == ModelTier::Local,
            RoutingMode::Cloud => tier == ModelTier::Cloud,
            RoutingMode::Off => false,
        }
    }

    async fn try_remote(&self) -> (AttemptOutcome, Option<Arc<dyn ModelClient>>) {
        let remote = &self.config.remote;
        if !remote.enabled {
            return (AttemptOutcome::Disabled, None);
        }
        let Some(endpoint) = &remote.endpoint else {
            return (
                AttemptOutcome::NotConfigured {
                    detail: "no address has been set for the other machine".to_string(),
                },
                None,
            );
        };

        match list_models(&endpoint.url, self.timeout()).await {
            Ok(models) => {
                // An empty model list means the server is up but has nothing
                // loaded, which is not a usable endpoint.
                let Some(model) = choose_model(&endpoint.model, &models) else {
                    return (
                        AttemptOutcome::Unreachable {
                            detail: format!("{} has no model loaded", endpoint.url),
                        },
                        None,
                    );
                };
                let token = secrets::get(SecretKind::RemoteEndpointToken);
                match OpenAiCompatibleClient::new(&endpoint.url, &model, token) {
                    Ok(client) => (
                        AttemptOutcome::Selected {
                            detail: format!("{model} on {}", endpoint.url),
                        },
                        Some(Arc::new(client)),
                    ),
                    Err(error) => (
                        AttemptOutcome::Unreachable {
                            detail: error.to_string(),
                        },
                        None,
                    ),
                }
            }
            Err(error) => (
                AttemptOutcome::Unreachable {
                    detail: error.to_string(),
                },
                None,
            ),
        }
    }

    async fn try_local(&self) -> (AttemptOutcome, Option<Arc<dyn ModelClient>>) {
        let local = &self.config.local;
        if !local.enabled {
            return (AttemptOutcome::Disabled, None);
        }
        if local.urls.is_empty() {
            return (
                AttemptOutcome::NotConfigured {
                    detail: "no local address is configured".to_string(),
                },
                None,
            );
        }

        let mut failures = Vec::new();
        for url in &local.urls {
            match list_models(url, self.timeout()).await {
                Ok(models) => match choose_model(&local.model, &models) {
                    Some(model) => match OpenAiCompatibleClient::new(url, &model, None) {
                        Ok(client) => {
                            return (
                                AttemptOutcome::Selected {
                                    detail: format!("{model} on {url}"),
                                },
                                Some(Arc::new(client)),
                            );
                        }
                        Err(error) => failures.push(format!("{url}: {error}")),
                    },
                    None => failures.push(format!("{url}: no model loaded")),
                },
                Err(error) => failures.push(format!("{url}: {error}")),
            }
        }

        (
            AttemptOutcome::Unreachable {
                detail: failures.join("; "),
            },
            None,
        )
    }

    fn try_cloud(&self) -> (AttemptOutcome, Option<Arc<dyn ModelClient>>) {
        let cloud = &self.config.cloud;
        if !cloud.enabled {
            return (AttemptOutcome::Disabled, None);
        }
        let Some(api_key) = secrets::get(SecretKind::CloudApiKey) else {
            return (
                AttemptOutcome::MissingCredential {
                    detail: format!(
                        "no API key stored; set one in settings or via {}",
                        SecretKind::CloudApiKey.env_var()
                    ),
                },
                None,
            );
        };
        if cloud.provider != "anthropic" {
            return (
                AttemptOutcome::NotConfigured {
                    detail: format!("provider `{}` is not supported yet", cloud.provider),
                },
                None,
            );
        }

        match AnthropicClient::new(&cloud.model, api_key) {
            Ok(client) => (
                AttemptOutcome::Selected {
                    detail: format!("{} via Anthropic", cloud.model),
                },
                Some(Arc::new(client)),
            ),
            Err(error) => (
                AttemptOutcome::Unreachable {
                    detail: error.to_string(),
                },
                None,
            ),
        }
    }
}

/// Pick which model to ask for from what a server actually offers.
///
/// A configured name wins if the server has it. If the configured name is
/// absent, using it anyway would fail at request time with a confusing error,
/// so fall through to what is actually there.
fn choose_model(configured: &str, available: &[String]) -> Option<String> {
    let configured = configured.trim();
    if !configured.is_empty() && available.iter().any(|model| model == configured) {
        return Some(configured.to_string());
    }
    if !configured.is_empty() && available.is_empty() {
        // Some servers do not implement the model list at all. Trusting the
        // configured name is better than refusing to try.
        return Some(configured.to_string());
    }
    available.first().cloned()
}

impl ModelRouter {
    /// Work through the tiers and return what was chosen, with the reasoning.
    pub async fn resolve(&self) -> Routing {
        let mut attempts = Vec::new();
        let mut chosen: Option<(ModelTier, Arc<dyn ModelClient>)> = None;

        for tier in [ModelTier::Remote, ModelTier::Local, ModelTier::Cloud] {
            // Once something has been chosen, the remaining tiers are still
            // recorded -- but as "not reached", not as failures they never had.
            if chosen.is_some() {
                break;
            }
            if !self.wanted(tier) {
                attempts.push(RouteAttempt {
                    tier,
                    outcome: AttemptOutcome::NotSelectedByUser,
                });
                continue;
            }

            let (outcome, client) = match tier {
                ModelTier::Remote => self.try_remote().await,
                ModelTier::Local => self.try_local().await,
                ModelTier::Cloud => self.try_cloud(),
            };

            if let Some(client) = client {
                chosen = Some((tier, client));
            }
            tracing::debug!(tier = tier.as_str(), outcome = %outcome.explain(), "routing");
            attempts.push(RouteAttempt { tier, outcome });
        }

        let (tier, client) = match chosen {
            Some((tier, client)) => (Some(tier), Some(client)),
            None => (None, None),
        };
        Routing {
            client,
            tier,
            attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::config::EndpointConfig;

    fn gpu(vram_gib: u64) -> GpuInfo {
        GpuInfo {
            name: "Test GPU".to_string(),
            vram_total_bytes: Some(vram_gib * GIB),
            vram_used_bytes: Some(0),
            driver_version: None,
        }
    }

    fn config_with_nothing_reachable() -> AiConfig {
        let mut config = AiConfig::default();
        // Addresses that nothing will answer on.
        config.local.urls = vec!["http://127.0.0.1:1/v1".to_string()];
        config
    }

    #[tokio::test]
    async fn with_nothing_available_the_router_reports_every_tier_it_tried() {
        let routing = ModelRouter::new(config_with_nothing_reachable())
            .resolve()
            .await;

        assert!(!routing.is_available());
        assert_eq!(routing.attempts.len(), 3);
        // Remote is off because nothing is configured; cloud is off by default.
        assert_eq!(routing.attempts[0].outcome, AttemptOutcome::Disabled);
        assert!(matches!(
            routing.attempts[1].outcome,
            AttemptOutcome::Unreachable { .. }
        ));
        assert_eq!(routing.attempts[2].outcome, AttemptOutcome::Disabled);
        assert!(routing.summary().contains("no model available"));
    }

    #[tokio::test]
    async fn turning_routing_off_tries_nothing_at_all() {
        let mut config = config_with_nothing_reachable();
        config.mode = RoutingMode::Off;

        let routing = ModelRouter::new(config).resolve().await;
        assert!(!routing.is_available());
        assert!(
            routing
                .attempts
                .iter()
                .all(|a| a.outcome == AttemptOutcome::NotSelectedByUser),
            "off must not contact anything"
        );
    }

    #[tokio::test]
    async fn pinning_a_tier_prevents_falling_back_to_another() {
        // Someone who pinned "local" must never have their diagnostics sent to
        // a cloud provider because the local server happened to be down.
        let mut config = config_with_nothing_reachable();
        config.mode = RoutingMode::Local;
        config.cloud.enabled = true;

        let routing = ModelRouter::new(config).resolve().await;
        assert!(!routing.is_available());

        let cloud = routing
            .attempts
            .iter()
            .find(|a| a.tier == ModelTier::Cloud)
            .unwrap();
        assert_eq!(cloud.outcome, AttemptOutcome::NotSelectedByUser);
    }

    #[tokio::test]
    async fn a_remote_endpoint_that_is_enabled_but_unset_says_so() {
        let mut config = config_with_nothing_reachable();
        config.remote.enabled = true;
        config.remote.endpoint = None;

        let routing = ModelRouter::new(config).resolve().await;
        assert!(matches!(
            routing.attempts[0].outcome,
            AttemptOutcome::NotConfigured { .. }
        ));
    }

    #[tokio::test]
    async fn an_unreachable_remote_falls_through_to_the_next_tier() {
        let mut config = config_with_nothing_reachable();
        config.remote.enabled = true;
        config.remote.endpoint = Some(EndpointConfig::new("http://127.0.0.1:2/v1", ""));

        let routing = ModelRouter::new(config).resolve().await;
        assert!(matches!(
            routing.attempts[0].outcome,
            AttemptOutcome::Unreachable { .. }
        ));
        // It went on to try local rather than giving up at the first failure.
        assert_eq!(routing.attempts[1].tier, ModelTier::Local);
    }

    #[test]
    fn cloud_is_the_only_tier_that_leaves_the_users_network() {
        assert!(ModelTier::Cloud.leaves_your_network());
        assert!(!ModelTier::Local.leaves_your_network());
        assert!(!ModelTier::Remote.leaves_your_network());
    }

    #[test]
    fn a_configured_model_wins_when_the_server_has_it() {
        let available = vec!["a".to_string(), "b".to_string()];
        assert_eq!(choose_model("b", &available).as_deref(), Some("b"));
    }

    #[test]
    fn a_configured_model_the_server_lacks_falls_back_to_what_is_there() {
        // Insisting on a missing name would fail later with a confusing error.
        let available = vec!["actually-loaded".to_string()];
        assert_eq!(
            choose_model("not-loaded", &available).as_deref(),
            Some("actually-loaded")
        );
    }

    #[test]
    fn a_server_that_lists_nothing_is_trusted_with_a_configured_name() {
        // Not every server implements the model list endpoint.
        assert_eq!(choose_model("my-model", &[]).as_deref(), Some("my-model"));
        assert_eq!(choose_model("", &[]), None);
    }

    #[test]
    fn vram_advice_scales_with_the_card() {
        assert!(advise_for_vram(&[gpu(6)]).recommendation.contains("7-8B"));
        assert!(advise_for_vram(&[gpu(24)]).recommendation.contains("70B"));
        assert!(
            advise_for_vram(&[gpu(2)])
                .recommendation
                .contains("another machine")
        );
    }

    #[test]
    fn unknown_vram_is_admitted_rather_than_guessed() {
        let advice = advise_for_vram(&[]);
        assert_eq!(advice.vram_bytes, None);
        assert!(advice.recommendation.contains("could not be determined"));
    }

    #[test]
    fn the_largest_card_is_the_one_that_matters() {
        let advice = advise_for_vram(&[gpu(6), gpu(24)]);
        assert_eq!(advice.vram_bytes, Some(24 * GIB));
    }
}
