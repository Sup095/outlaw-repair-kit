//! Linux implementation of the platform layer.

use crate::Result;
use crate::platform::{
    DeviceIssue, HostInfo, LogRecord, MemoryInfo, Platform, PlatformKind, ProcessInfo, Volume,
    VolumeRole, common, linux_devices, linux_journal,
};

pub struct LinuxPlatform {
    _private: (),
}

impl LinuxPlatform {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for LinuxPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Linux
    }

    fn host(&self) -> Result<HostInfo> {
        common::host_info()
    }

    fn volumes(&self) -> Result<Vec<Volume>> {
        let mut volumes = common::raw_volumes()?;
        for volume in &mut volumes {
            // The root filesystem is the one that breaks the machine when it
            // fills. A separate /home or /var is ordinary data storage, even
            // though filling those is still worth flagging.
            if volume.role == VolumeRole::Data && volume.mount_point == "/" {
                volume.role = VolumeRole::System;
            }
        }
        Ok(volumes)
    }

    fn processes(&self) -> Result<Vec<ProcessInfo>> {
        common::processes()
    }

    fn device_issues(&self) -> Result<Vec<DeviceIssue>> {
        linux_devices::device_issues()
    }

    fn memory(&self) -> Result<MemoryInfo> {
        common::memory_info()
    }

    fn recent_log_errors(&self, since: std::time::Duration) -> Result<Vec<LogRecord>> {
        linux_journal::recent_errors(since)
    }
}
