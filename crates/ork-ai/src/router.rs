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

// The bands below are all multiples of GIB, so they say GiB. They used to
// say "GB", which put "24 GiB of video memory" and "24 GB or more of video
// memory" on adjacent lines of `outlaw models`, about the same card. Both
// are defensible alone -- the card holds 24 GiB and the shops call it a
// 24 GB card -- and together they read as a tool that cannot keep its own
// units straight, on the screen where somebody is deciding whether to
// believe its numbers.

pub fn advise_for_vram(gpus: &[GpuInfo]) -> VramAdvice {
    let vram_bytes = gpus.iter().filter_map(|gpu| gpu.vram_total_bytes).max();

    let recommendation = match vram_bytes {
        None => "Video memory could not be determined, so no size can be recommended. \
                 Install the graphics vendor's tools, or choose a model yourself."
            .to_string(),
        Some(bytes) if bytes < 4 * GIB => {
            "Under 4 GiB of video memory. A local model will be slow and heavily limited; \
             consider using a model on another machine instead."
                .to_string()
        }
        Some(bytes) if bytes < 8 * GIB => {
            "Around 6 GiB of video memory. A 7-8B model at 4-bit quantisation is the \
             practical ceiling. Larger analysis is better sent to another machine."
                .to_string()
        }
        Some(bytes) if bytes < 16 * GIB => {
            "8-12 GiB of video memory. A 13-14B model at 4-bit quantisation fits \
             comfortably."
                .to_string()
        }
        Some(bytes) if bytes < 24 * GIB => {
            "16-20 GiB of video memory. A 30B-class model at 4-bit quantisation fits.".to_string()
        }
        Some(_) => "24 GiB or more of video memory. A 70B-class model at 4-bit quantisation \
                    fits, and this machine is a good candidate for serving other machines \
                    on the network."
            .to_string(),
    };

    VramAdvice {
        vram_bytes,
        recommendation,
    }
}

/// A model this machine could actually run, named.
///
/// [`advise_for_vram`] says what size of model fits and stops there, because
/// the tool does not load models and should not pretend to. This goes one step
/// further and names one, for the single case where naming one is the useful
/// thing: an installer offering to fetch a model on somebody's behalf has to
/// know which model to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPick {
    /// What to ask Ollama for.
    pub tag: &'static str,
    /// Roughly how much will be downloaded, in whole gigabytes, so that the
    /// size can be said out loud before anybody commits to it. Approximate on
    /// purpose -- quantisations differ, and a number presented to two decimal
    /// places would be a precision this does not have.
    ///
    /// Gigabytes, deliberately, while video memory beside it is measured in
    /// gibibytes. They are different quantities: this is how much comes down
    /// the wire, which everything that publishes model sizes quotes in GB, and
    /// that is a size somebody will compare against their connection rather
    /// than against their card. Not the same mistake as describing one card in
    /// two units.
    pub about_gb: u32,
    /// Why this one and not a larger one.
    pub why: &'static str,
}

/// Choose a model to offer, given however much video memory was found.
///
/// Sized so the model fits *with room for its context*, rather than being the
/// largest one that technically loads. A model that fills the card and then
/// stalls the machine the first time it is asked a real question is worse than
/// a smaller one that answers.
///
/// `None` for video memory means none was found, which is not the same as
/// none being present -- it usually means the vendor's tools are not
/// installed. Either way the answer is the same: pick something that will run
/// on the processor.
///
/// **The two shell installers carry this same table.** They cannot call this,
/// being shell, so `install/install.ps1` and `install/install.sh` mirror it by
/// hand and a test below pins the thresholds so a change here is visible.
pub fn model_for_vram(vram_bytes: Option<u64>) -> ModelPick {
    let gib = vram_bytes.map(|bytes| bytes / GIB).unwrap_or(0);

    if gib >= 22 {
        ModelPick {
            tag: "qwen3:32b",
            about_gb: 20,
            why: "There is room for a 32B model and its context.",
        }
    } else if gib >= 14 {
        ModelPick {
            tag: "qwen3:14b",
            about_gb: 9,
            why: "A 14B model fits with room to think.",
        }
    } else if gib >= 10 {
        ModelPick {
            tag: "qwen3:8b",
            about_gb: 5,
            why: "An 8B model leaves enough spare for a long question.",
        }
    } else if gib >= 6 {
        ModelPick {
            tag: "qwen3:4b",
            about_gb: 3,
            why: "A 4B model is what this card will hold comfortably.",
        }
    } else {
        ModelPick {
            tag: "qwen3:1.7b",
            about_gb: 2,
            why: "Small enough to run on the processor, which is what it will                   be doing without a graphics card to hold it.",
        }
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
                // A linked machine names its own credential; a hand-typed
                // endpoint falls back to the single stored remote token.
                let token = endpoint
                    .token_ref
                    .as_deref()
                    .and_then(secrets::get_named)
                    .or_else(|| secrets::get(SecretKind::RemoteEndpointToken));
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

/// Fragments that mark a model as one that turns text into vectors rather than
/// into sentences.
///
/// A hand-maintained list matching on names, which is a guess and not an
/// identification -- the same caveat as every other name list in this project.
/// The difference is what being wrong costs. Wrongly skipping a model that
/// could have answered means picking a different one, and it only ever happens
/// when there is a different one to pick. Wrongly *choosing* one of these
/// means the tool has no explanations at all, and says so with a server error
/// about chat not being supported, which reads as the tool being broken.
///
/// Rerankers are here for the same reason: they score pairs of texts and
/// cannot hold a conversation either.
const NOT_FOR_ANSWERING: &[&str] = &[
    "embed",
    "embedding",
    "all-minilm",
    "bge-",
    "gte-",
    "e5-",
    "rerank",
];

/// Whether this name looks like something that cannot answer a question.
fn cannot_answer_questions(model: &str) -> bool {
    let name = model.to_ascii_lowercase();
    NOT_FOR_ANSWERING.iter().any(|mark| name.contains(mark))
}

/// Pick which model to ask for from what a server actually offers.
///
/// A configured name wins if the server has it -- always, even if it looks
/// like an embedding model, because an explicit choice is a decision and this
/// is not the place to overrule one.
///
/// Otherwise the first one that could plausibly answer a question. Found by
/// running the live test on a real machine: Ollama lists alphabetically, the
/// machine had `nomic-embed-text` alongside three perfectly good chat models,
/// and taking the first meant every explanation came back as
/// *"nomic-embed-text does not support chat"*. The tool degraded honestly,
/// which is right, and it degraded when it did not have to, which is not.
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
    available
        .iter()
        .find(|model| !cannot_answer_questions(model))
        // If every name looks like an embedding model, ask the first one
        // anyway. A name is a guess; refusing to try on the strength of a
        // guess would turn "we think none of these can answer" into "you have
        // no models", which is a claim about the machine that the tool cannot
        // make from a list of names.
        .or_else(|| available.first())
        .cloned()
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

    #[test]
    fn the_model_offered_matches_the_table_the_shell_installers_carry() {
        // `install/install.ps1` and `install/install.sh` cannot call this, so
        // they mirror it by hand. These are the exact thresholds they use, in
        // whole gigabytes. If this test is changed, both scripts change too --
        // an installer that offers a different model depending on which
        // installer you used is a bug people would report as a mystery.
        let gb = |n: u64| Some(n * GIB);
        assert_eq!(model_for_vram(gb(24)).tag, "qwen3:32b");
        assert_eq!(model_for_vram(gb(22)).tag, "qwen3:32b");
        assert_eq!(model_for_vram(gb(21)).tag, "qwen3:14b");
        assert_eq!(model_for_vram(gb(16)).tag, "qwen3:14b");
        assert_eq!(model_for_vram(gb(12)).tag, "qwen3:8b");
        assert_eq!(model_for_vram(gb(8)).tag, "qwen3:4b");
        assert_eq!(model_for_vram(gb(4)).tag, "qwen3:1.7b");
    }

    #[test]
    fn no_graphics_card_still_gets_an_answer() {
        // Not knowing how much video memory there is usually means the
        // vendor's tools are missing, not that there is no card. Either way
        // the safe offer is the one that runs on the processor.
        let unknown = model_for_vram(None);
        assert_eq!(unknown.tag, "qwen3:1.7b");
        assert_eq!(unknown, model_for_vram(Some(0)));
        assert!(unknown.why.contains("processor"), "{}", unknown.why);
    }

    #[test]
    fn every_offer_admits_roughly_how_big_it_is() {
        // Somebody on a metered connection is entitled to know that "yes" here
        // means several gigabytes before they press it.
        for vram in [
            None,
            Some(4 * GIB),
            Some(8 * GIB),
            Some(16 * GIB),
            Some(24 * GIB),
        ] {
            let pick = model_for_vram(vram);
            assert!(pick.about_gb > 0, "{} claims no size", pick.tag);
            assert!(!pick.why.is_empty());
        }
    }
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
    fn an_embedding_model_is_not_picked_over_one_that_can_answer() {
        // The real case, from a real machine. Ollama lists alphabetically, so
        // `nomic-embed-text` came first and every explanation came back as
        // "nomic-embed-text does not support chat". Three usable models were
        // sitting right there.
        let available = vec![
            "nomic-embed-text:latest".to_string(),
            "gemma4:e2b".to_string(),
            "gemma4:e4b".to_string(),
            "qwen3-coder:30b".to_string(),
        ];
        assert_eq!(
            choose_model("", &available).as_deref(),
            Some("gemma4:e2b"),
            "the first model that can hold a conversation, not the first model"
        );
    }

    #[test]
    fn an_explicit_choice_is_never_overruled() {
        // Somebody who names an embedding model has made a decision, possibly
        // for a reason this code does not know. It is not this function's job
        // to argue, and a tool that quietly used a different model than the
        // one it was told to would be worse than one that fails.
        let available = vec!["nomic-embed-text".to_string(), "gemma4:e2b".to_string()];
        assert_eq!(
            choose_model("nomic-embed-text", &available).as_deref(),
            Some("nomic-embed-text")
        );
    }

    #[test]
    fn a_server_with_only_embedding_models_is_still_asked() {
        // A name is a guess, not an identification. Refusing to try would turn
        // "these names look like embedders" into "you have no models", which
        // is a claim about somebody's machine that a list of names cannot
        // support. Ask, and let the server be the one to say no.
        let available = vec!["nomic-embed-text".to_string(), "bge-m3".to_string()];
        assert_eq!(
            choose_model("", &available).as_deref(),
            Some("nomic-embed-text")
        );
    }

    #[test]
    fn the_names_that_are_skipped_are_the_ones_meant_to_be_skipped() {
        for embedder in [
            "nomic-embed-text:latest",
            "mxbai-embed-large",
            "all-minilm:l6-v2",
            "bge-m3",
            "gte-large",
            "e5-mistral-7b-instruct",
            "bge-reranker-v2-m3",
            "NOMIC-EMBED-TEXT",
        ] {
            assert!(
                cannot_answer_questions(embedder),
                "{embedder} should not be chosen to answer a question"
            );
        }
        // And nothing that can answer is caught by accident. `gemma`,
        // `qwen`, `llama` and friends must all survive -- a list that swept
        // up real chat models would leave somebody with no explanations for
        // the opposite reason.
        for chatty in [
            "gemma4:e2b",
            "qwen3-coder:30b",
            "llama3.3:70b",
            "mistral-small",
            "phi4",
            "deepseek-r1:14b",
            "gpt-4o-mini",
            "claude-opus-5",
        ] {
            assert!(
                !cannot_answer_questions(chatty),
                "{chatty} can answer questions and must not be skipped"
            );
        }
    }

    #[test]
    fn the_advice_is_labelled_with_the_unit_it_was_measured_in() {
        // Every band is a multiple of GIB, so every band says GiB. This is not
        // pedantry about a seven per cent difference: `outlaw models` prints
        // the measured figure and the advice one line apart, and it said
        // "24 GiB of video memory" directly above "24 GB or more of video
        // memory" about the same card.
        for card in [1, 6, 10, 18, 24, 48] {
            let said = advise_for_vram(&[gpu(card)]).recommendation;
            assert!(
                !said.replace("GiB", "").contains("GB"),
                "{card} GiB card was advised in GB: {said:?}"
            );
        }
        // And the unknown case still says something rather than a unit.
        assert!(
            advise_for_vram(&[])
                .recommendation
                .contains("could not be determined")
        );
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
