use serde::{Deserialize, Serialize};

/// How much of the system a scan covers.
///
/// Tiers are ordered, and a probe declares the *lowest* tier it participates
/// in -- so everything in `Quick` also runs during `Full` and `Deep`.
///
/// Deliberately absent: any notion of a duration. No tier and no individual
/// check is given a deadline. Long-running work is supervised by a liveness
/// check (is this process still doing anything?) and is always manually
/// cancellable, but it is never cut off for taking too long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanTier {
    /// Running processes, disk space, recent log errors, driver/package
    /// version sanity, and smoke-test launches of key applications.
    Quick,
    /// Everything in `Quick`, plus a full driver/package conflict audit, SMART
    /// on every drive, the complete log history, a full malware scan, and
    /// launch tests across everything installed.
    Full,
    /// Everything in `Full`, plus stress-testing and burn-in of CPU, memory,
    /// and GPU, an exhaustive rootkit scan, and system file checksum
    /// verification.
    ///
    /// **Nothing declares this tier yet.** The variant exists and is wired all
    /// the way through, but no probe has `min_tier: Deep`, so a deep scan runs
    /// exactly what a full one runs. Both front-ends say so where the tier is
    /// chosen, rather than letting somebody pick it and wonder why it finished
    /// so quickly.
    Deep,
}

impl ScanTier {
    /// Every tier, in increasing order of coverage.
    pub const ALL: [ScanTier; 3] = [ScanTier::Quick, ScanTier::Full, ScanTier::Deep];

    /// Whether a scan at this tier should run a probe whose minimum tier is
    /// `probe_min`.
    pub fn includes(self, probe_min: ScanTier) -> bool {
        self >= probe_min
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ScanTier::Quick => "quick",
            ScanTier::Full => "full",
            ScanTier::Deep => "deep",
        }
    }
}

impl std::fmt::Display for ScanTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ScanTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "quick" => Ok(ScanTier::Quick),
            "full" => Ok(ScanTier::Full),
            "deep" => Ok(ScanTier::Deep),
            other => Err(format!(
                "unknown scan tier `{other}` (expected quick, full, or deep)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_tiers_include_lower_probes() {
        assert!(ScanTier::Deep.includes(ScanTier::Quick));
        assert!(ScanTier::Full.includes(ScanTier::Quick));
        assert!(ScanTier::Quick.includes(ScanTier::Quick));
        assert!(!ScanTier::Quick.includes(ScanTier::Full));
    }
}
