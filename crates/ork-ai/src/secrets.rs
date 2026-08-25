//! API keys, kept in the operating system's credential store.
//!
//! Keys never touch the configuration file. That file is plain text, it gets
//! copied into bug reports and pasted into forums, and a format that invites
//! secrets into it will leak them eventually. Instead this uses the platform's
//! own credential store -- Credential Manager on Windows, the desktop secret
//! service on Linux -- which is encrypted at rest and tied to the user account.
//!
//! Everything here degrades gracefully. A headless Linux box with no secret
//! service running is a perfectly reasonable place to run a diagnostic tool,
//! and it should report that it cannot store a key rather than refusing to
//! start.

use anyhow::Context;

use crate::Result;

/// Service name recorded in the credential store, so a person can find and
/// remove these entries themselves.
const SERVICE: &str = "outlaw-repair-kit";

/// Which stored credential is being asked for.
///
/// An enum rather than a free-form string so that a typo cannot silently
/// create a second, empty credential that appears to be missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    /// API key for the configured cloud provider.
    CloudApiKey,
    /// Optional bearer token for a remote endpoint that requires one.
    RemoteEndpointToken,
}

impl SecretKind {
    fn account(self) -> &'static str {
        match self {
            SecretKind::CloudApiKey => "cloud-api-key",
            SecretKind::RemoteEndpointToken => "remote-endpoint-token",
        }
    }

    /// How to describe this to a person.
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::CloudApiKey => "cloud provider API key",
            SecretKind::RemoteEndpointToken => "remote endpoint token",
        }
    }

    /// Environment variable checked before the credential store.
    ///
    /// This exists for containers, CI, and headless machines, where there is
    /// no credential store to talk to.
    pub fn env_var(self) -> &'static str {
        match self {
            SecretKind::CloudApiKey => "ORK_CLOUD_API_KEY",
            SecretKind::RemoteEndpointToken => "ORK_REMOTE_TOKEN",
        }
    }
}

fn entry(kind: SecretKind) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, kind.account()).with_context(|| {
        format!(
            "could not reach the credential store for the {}",
            kind.label()
        )
    })
}

/// Fetch a secret, or `None` if it has not been set.
///
/// The environment variable wins over the credential store, so a container or
/// a CI run can supply a key without a desktop session.
pub fn get(kind: SecretKind) -> Option<String> {
    if let Ok(value) = std::env::var(kind.env_var())
        && !value.trim().is_empty()
    {
        tracing::debug!(secret = kind.account(), "using value from the environment");
        return Some(value);
    }

    match entry(kind).and_then(|entry| entry.get_password().map_err(Into::into)) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        Ok(_) => None,
        Err(error) => {
            // A missing entry and an unavailable credential store are both
            // "no key", but only one of them is worth mentioning.
            tracing::debug!(secret = kind.account(), %error, "no stored credential");
            None
        }
    }
}

/// Whether a secret is available, without reading its value.
pub fn is_set(kind: SecretKind) -> bool {
    get(kind).is_some()
}

/// Store a secret. This is what the settings screen calls.
pub fn set(kind: SecretKind, value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.trim().is_empty(),
        "the {} cannot be empty",
        kind.label()
    );
    entry(kind)?
        .set_password(value)
        .with_context(|| format!("could not save the {}", kind.label()))?;
    tracing::debug!(secret = kind.account(), "credential stored");
    Ok(())
}

/// Remove a stored secret. Removing one that is not there is not an error.
pub fn delete(kind: SecretKind) -> Result<()> {
    match entry(kind)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not remove the {}", kind.label())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_secret_is_rejected_rather_than_stored() {
        // Storing an empty key would produce a configuration that looks set up
        // and fails at the moment it is used.
        assert!(set(SecretKind::CloudApiKey, "").is_err());
        assert!(set(SecretKind::CloudApiKey, "   ").is_err());
    }

    #[test]
    fn the_environment_overrides_the_credential_store() {
        // SAFETY: this is the documented way to set an environment variable,
        // and these tests are the only reader of this particular variable.
        unsafe { std::env::set_var(SecretKind::CloudApiKey.env_var(), "from-environment") };
        assert_eq!(
            get(SecretKind::CloudApiKey).as_deref(),
            Some("from-environment")
        );
        unsafe { std::env::remove_var(SecretKind::CloudApiKey.env_var()) };
    }

    #[test]
    fn a_blank_environment_value_is_treated_as_unset() {
        unsafe { std::env::set_var(SecretKind::RemoteEndpointToken.env_var(), "  ") };
        // Falls through to the credential store, which has nothing in a test
        // environment.
        assert_eq!(get(SecretKind::RemoteEndpointToken), None);
        unsafe { std::env::remove_var(SecretKind::RemoteEndpointToken.env_var()) };
    }

    #[test]
    fn each_secret_has_its_own_slot_and_label() {
        assert_ne!(
            SecretKind::CloudApiKey.account(),
            SecretKind::RemoteEndpointToken.account()
        );
        assert_ne!(
            SecretKind::CloudApiKey.env_var(),
            SecretKind::RemoteEndpointToken.env_var()
        );
        assert!(!SecretKind::CloudApiKey.label().is_empty());
    }
}
