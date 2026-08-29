//! User configuration.
//!
//! Two rules shape this module.
//!
//! First, **no secrets live here.** API keys go in the operating system's
//! credential store. This file is plain text that people paste into bug
//! reports, and a configuration format that invites secrets into it will get
//! them leaked eventually.
//!
//! Second, **every setting has a working default.** The tool has to do
//! something sensible on a machine that has never been configured, because
//! most people will never open the settings at all.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::Result;

/// Which model the router should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingMode {
    /// Work down the preference order until something answers.
    #[default]
    Auto,
    /// Only ever use the configured remote endpoint.
    Remote,
    /// Only ever use a local model.
    Local,
    /// Only ever use the cloud provider.
    Cloud,
    /// Do not use a model at all. Deterministic checks and the runbook library
    /// still work; nothing is sent anywhere.
    Off,
}

/// A model endpoint that speaks the OpenAI wire format.
///
/// LM Studio, Ollama, vLLM, and llama.cpp's server all speak it, which is why
/// "local model" and "a model on another machine" are the same code path with
/// a different address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointConfig {
    /// Base URL, e.g. `http://100.64.0.2:1234/v1`.
    pub url: String,
    /// Model name to request. Empty means "whatever the server offers first".
    #[serde(default)]
    pub model: String,
    /// Which stored credential proves the right to use this endpoint.
    ///
    /// A name, never a token: this file is not a place for secrets. A linked
    /// machine puts its own credential's name here so that a link and a
    /// hand-typed endpoint end up on exactly the same code path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_ref: Option<String>,
}

impl EndpointConfig {
    pub fn new(url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            model: model.into(),
            token_ref: None,
        }
    }

    /// The same endpoint, reached with a named credential.
    pub fn with_token_ref(mut self, account: impl Into<String>) -> Self {
        self.token_ref = Some(account.into());
        self
    }
}

/// The remote endpoint on another machine, typically over a private network.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Off until someone sets it up, since there is no sensible default
    /// address to guess.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<EndpointConfig>,
}

/// A model running on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Base URLs to try, in order. The defaults cover the two most common
    /// local servers on their usual ports.
    #[serde(default = "default_local_urls")]
    pub urls: Vec<String>,
    /// Model to request. Empty means "choose one that fits the graphics memory
    /// this machine actually has".
    #[serde(default)]
    pub model: String,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            urls: default_local_urls(),
            model: String::new(),
        }
    }
}

/// A hosted model, reached over the internet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudConfig {
    /// Off by default, and deliberately so.
    ///
    /// The other two tiers keep diagnostic data on hardware the user owns.
    /// This one sends it to a third party, so it is something a person turns
    /// on knowingly rather than something that happens because the local model
    /// was not running.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_cloud_provider")]
    pub provider: String,
    #[serde(default = "default_cloud_model")]
    pub model: String,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_cloud_provider(),
            model: default_cloud_model(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_local_urls() -> Vec<String> {
    vec![
        // LM Studio's default.
        "http://127.0.0.1:1234/v1".to_string(),
        // Ollama's default, which also serves an OpenAI-compatible API.
        "http://127.0.0.1:11434/v1".to_string(),
    ]
}

fn default_cloud_provider() -> String {
    "anthropic".to_string()
}

fn default_cloud_model() -> String {
    "claude-opus-5".to_string()
}

/// Everything about how the tool uses a model.
///
/// `Default` is written out rather than derived. A derived implementation
/// would give every numeric field Rust's zero default, which is not the same
/// as the value serde fills in for a missing field -- and the two disagreeing
/// means the tool behaves differently depending on whether a configuration
/// file happens to exist. The test at the bottom of this file holds them
/// together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub mode: RoutingMode,
    #[serde(default)]
    pub remote: RemoteConfig,
    #[serde(default)]
    pub local: LocalConfig,
    #[serde(default)]
    pub cloud: CloudConfig,
    /// How long to wait for an endpoint to answer a reachability check.
    ///
    /// This is a *connection* check, not a limit on how long the model may
    /// think. Deciding whether a machine is switched on is exactly the kind of
    /// question that deserves a short answer.
    #[serde(default = "default_reachability_ms")]
    pub reachability_timeout_ms: u64,
}

fn default_reachability_ms() -> u64 {
    2000
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            mode: RoutingMode::default(),
            remote: RemoteConfig::default(),
            local: LocalConfig::default(),
            cloud: CloudConfig::default(),
            reachability_timeout_ms: default_reachability_ms(),
        }
    }
}

/// Settings for looking at what is running.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Programs to leave alone, by name, whatever else the tool decides.
    ///
    /// Matched without regard to case, because somebody typing a program's
    /// name in will not match the capitalisation the operating system reports,
    /// and being silently ignored is the worst possible outcome for a setting
    /// whose entire meaning is "leave this one alone".
    #[serde(default)]
    pub pinned: Vec<String>,
}

impl ProcessConfig {
    /// Whether this program is on the leave-alone list.
    ///
    /// The comparison is the same one the classifier makes, so a program that
    /// reads as pinned here is pinned there. Two answers to "is this pinned"
    /// would be worse than none.
    pub fn is_pinned(&self, name: &str) -> bool {
        let wanted = name.trim().to_ascii_lowercase();
        self.pinned
            .iter()
            .any(|held| held.trim().to_ascii_lowercase() == wanted)
    }

    /// Add a program to the leave-alone list.
    ///
    /// Returns whether anything changed, so a caller can avoid writing a file
    /// that would come out identical. Keeps the name as it was given rather
    /// than lower-casing it: the list is read by people, and a settings file
    /// that quietly rewrote `Steam.exe` as `steam.exe` would look like the
    /// tool had misunderstood.
    ///
    /// An empty name is refused rather than stored. An empty string in this
    /// list would match nothing and read, on the screen, as a rule that was
    /// there.
    pub fn pin(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.is_pinned(name) {
            return false;
        }
        self.pinned.push(name.to_string());
        true
    }

    /// Take a program off the leave-alone list, however it was capitalised
    /// when it went on.
    pub fn unpin(&mut self, name: &str) -> bool {
        let wanted = name.trim().to_ascii_lowercase();
        let before = self.pinned.len();
        self.pinned
            .retain(|held| held.trim().to_ascii_lowercase() != wanted);
        self.pinned.len() != before
    }
}

#[cfg(test)]
mod pinning {
    use super::ProcessConfig;

    fn with(names: &[&str]) -> ProcessConfig {
        ProcessConfig {
            pinned: names.iter().map(|name| name.to_string()).collect(),
        }
    }

    #[test]
    fn a_program_is_recognised_however_it_was_typed() {
        // The whole point of the setting is "leave this one alone", and being
        // silently ignored because of a capital letter is the worst possible
        // outcome for it. The classifier already matches this way; so does
        // this, or the window would show a program as unpinned while the
        // classifier held it back.
        let held = with(&["Steam.exe"]);
        for typed in ["Steam.exe", "steam.exe", "STEAM.EXE", "  steam.exe  "] {
            assert!(held.is_pinned(typed), "{typed} should read as pinned");
        }
        assert!(!held.is_pinned("steamwebhelper.exe"));
    }

    #[test]
    fn pinning_the_same_program_twice_changes_nothing() {
        let mut held = with(&["Steam.exe"]);
        assert!(!held.pin("steam.exe"), "already pinned, differently typed");
        assert_eq!(held.pinned, vec!["Steam.exe"]);
        assert!(held.pin("obs64.exe"));
        assert_eq!(held.pinned.len(), 2);
    }

    #[test]
    fn the_name_is_kept_as_it_was_given() {
        // A settings file that rewrote what somebody typed would read as the
        // tool having misunderstood them, and this is a file people open.
        let mut held = ProcessConfig::default();
        held.pin("Steam.exe");
        assert_eq!(held.pinned, vec!["Steam.exe"]);
    }

    #[test]
    fn unpinning_works_whatever_case_it_went_in_as() {
        let mut held = with(&["Steam.exe", "obs64.exe"]);
        assert!(held.unpin("STEAM.EXE"));
        assert_eq!(held.pinned, vec!["obs64.exe"]);
        assert!(!held.unpin("steam.exe"), "already gone");
    }

    #[test]
    fn nothing_is_pinned_by_an_empty_name() {
        // A blank in the list matches nothing and reads, on a screen, as a
        // rule that is there. Refused where it is cheapest to refuse.
        let mut held = ProcessConfig::default();
        assert!(!held.pin(""));
        assert!(!held.pin("   "));
        assert!(held.pinned.is_empty());
    }
}

/// The whole configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub processes: ProcessConfig,
}

impl Config {
    /// Where the configuration file lives on this platform.
    ///
    /// `%APPDATA%\outlaw-repair-kit\config.toml` on Windows, and
    /// `~/.config/outlaw-repair-kit/config.toml` on Linux.
    pub fn default_path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("could not determine this system's configuration directory")?;
        Ok(dir.join("outlaw-repair-kit").join("config.toml"))
    }

    /// Load configuration, falling back to defaults when there is no file.
    ///
    /// A missing file is the normal state for a fresh install, not an error.
    pub fn load_or_default(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Self::parse(&text).with_context(|| format!("could not read {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(path = %path.display(), "no configuration file; using defaults");
                Ok(Self::default())
            }
            Err(error) => Err(error).with_context(|| format!("could not open {}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("the configuration file is not valid TOML")
    }

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("could not serialise the configuration")
    }

    /// Write the configuration, creating the directory if needed.
    ///
    /// This is what the settings screen calls. It writes to a temporary file
    /// and renames it into place, so an interrupted save cannot leave the user
    /// with a truncated configuration and a tool that will not start.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let text = self.to_toml()?;
        let temporary = path.with_extension("toml.tmp");
        std::fs::write(&temporary, text)
            .with_context(|| format!("could not write {}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .with_context(|| format!("could not replace {}", path.display()))?;
        tracing::debug!(path = %path.display(), "configuration saved");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_yields_working_defaults() {
        let config = Config::parse("").expect("empty config should parse");
        assert_eq!(config.ai.mode, RoutingMode::Auto);
        assert!(
            config.ai.local.enabled,
            "a local model should be tried by default"
        );
        assert!(
            !config.ai.remote.enabled,
            "there is no address to guess for a remote"
        );
        assert!(
            !config.ai.cloud.enabled,
            "sending diagnostics to a third party must be opted into, never defaulted on"
        );
        assert!(!config.ai.local.urls.is_empty());
    }

    #[test]
    fn a_partial_file_keeps_defaults_for_everything_it_omits() {
        // People hand-edit this file and write only the line they care about.
        // That must not silently reset everything else.
        let config = Config::parse("[ai]\nmode = \"local\"\n").expect("should parse");
        assert_eq!(config.ai.mode, RoutingMode::Local);
        assert!(config.ai.local.enabled);
        assert_eq!(config.ai.reachability_timeout_ms, 2000);
    }

    #[test]
    fn the_built_in_defaults_match_what_an_empty_file_produces() {
        // These are two separate code paths -- Rust's Default and serde's
        // per-field defaults -- and they must not drift apart. When they do,
        // the tool behaves differently depending on whether a configuration
        // file happens to exist, which is close to impossible to debug from
        // the outside.
        assert_eq!(Config::default(), Config::parse("").unwrap());
    }

    #[test]
    fn the_reachability_timeout_is_long_enough_for_a_real_network() {
        // A remote endpoint is typically reached over a private network link
        // to another machine. A sub-second budget would report a perfectly
        // healthy machine as unreachable.
        assert!(
            Config::default().ai.reachability_timeout_ms >= 1000,
            "got {}ms",
            Config::default().ai.reachability_timeout_ms
        );
    }

    #[test]
    fn configuration_survives_a_round_trip() {
        let mut original = Config::default();
        original.ai.mode = RoutingMode::Remote;
        original.ai.remote.enabled = true;
        original.ai.remote.endpoint =
            Some(EndpointConfig::new("http://example:1234/v1", "some-model"));

        let text = original.to_toml().expect("should serialise");
        let parsed = Config::parse(&text).expect("should parse back");
        assert_eq!(original, parsed);
    }

    #[test]
    fn a_broken_file_reports_the_problem_rather_than_silently_resetting() {
        // Silently falling back to defaults would leave someone wondering why
        // their configured endpoint is being ignored.
        let result = Config::parse("this is not toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn saving_and_loading_round_trips_through_a_real_file() {
        let dir = std::env::temp_dir().join(format!("ork-config-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        // A path that does not exist yet is the fresh-install case.
        let loaded = Config::load_or_default(&path).expect("missing file should be fine");
        assert_eq!(loaded, Config::default());

        let mut config = Config::default();
        config.ai.cloud.enabled = true;
        config.save(&path).expect("should save");

        let reloaded = Config::load_or_default(&path).expect("should load");
        assert_eq!(reloaded, config);
        // The temporary file used during the write must not be left behind.
        assert!(!path.with_extension("toml.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
