//! Windows implementation of the platform layer.

use crate::Result;
use crate::platform::{
    DeviceIssue, HostInfo, LogRecord, MemoryInfo, Platform, PlatformKind, ProcessInfo, Volume,
    VolumeRole, common, win_devices, win_eventlog,
};

pub struct WindowsPlatform {
    /// The drive the running OS lives on, e.g. `C:`. Read once at startup.
    system_drive: String,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        // SystemDrive is set by the OS itself on every Windows install. The
        // fallback only matters in a stripped environment.
        let system_drive = std::env::var("SystemDrive")
            .unwrap_or_else(|_| "C:".to_string())
            .trim_end_matches(std::path::MAIN_SEPARATOR)
            .to_ascii_uppercase();
        Self { system_drive }
    }

    fn is_system_volume(&self, mount_point: &str) -> bool {
        mount_point
            .trim_end_matches(std::path::MAIN_SEPARATOR)
            .eq_ignore_ascii_case(&self.system_drive)
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for WindowsPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn host(&self) -> Result<HostInfo> {
        common::host_info()
    }

    fn volumes(&self) -> Result<Vec<Volume>> {
        let mut volumes = common::raw_volumes()?;
        for volume in &mut volumes {
            if volume.role == VolumeRole::Data && self.is_system_volume(&volume.mount_point) {
                volume.role = VolumeRole::System;
            }
        }
        Ok(volumes)
    }

    fn processes(&self) -> Result<Vec<ProcessInfo>> {
        common::processes()
    }

    fn device_issues(&self) -> Result<Vec<DeviceIssue>> {
        win_devices::device_issues()
    }

    fn memory(&self) -> Result<MemoryInfo> {
        common::memory_info()
    }

    fn recent_log_errors(&self, since: std::time::Duration) -> Result<Vec<LogRecord>> {
        win_eventlog::recent_errors(since)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_drive_matches_regardless_of_trailing_separator_or_case() {
        let platform = WindowsPlatform {
            system_drive: "C:".to_string(),
        };
        assert!(platform.is_system_volume("C:\\"));
        assert!(platform.is_system_volume("c:"));
        assert!(!platform.is_system_volume("D:\\"));
    }
}
