//! Implementations shared by every platform.
//!
//! Anything in here is genuinely OS-independent -- either pure logic, or a
//! cross-platform crate that already does the OS-specific work correctly. The
//! platform implementations delegate here and then apply their own
//! corrections, which keeps the abstraction seam intact without duplicating
//! work that a maintained crate already does well.

use std::path::Path;

use sysinfo::{Disks, System};

use anyhow::Context;

use crate::Result;
use crate::platform::{HostInfo, MemoryInfo, ProcessInfo, Volume, VolumeRole};

/// Filesystem types that exist in the mount table but are not real storage.
/// Reporting on these produces nothing but noise -- `tmpfs` sitting at 100%
/// is normal, not a problem.
const PSEUDO_FILESYSTEMS: &[&str] = &[
    "autofs",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fuse.gvfsd-fuse",
    "fuse.portal",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "overlay",
    "proc",
    "pstore",
    "ramfs",
    "securityfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

fn is_pseudo_filesystem(fs: &str) -> bool {
    let fs = fs.to_ascii_lowercase();
    PSEUDO_FILESYSTEMS.iter().any(|candidate| *candidate == fs)
}

/// Collect basic machine identification.
pub fn host_info() -> Result<HostInfo> {
    let mut system = System::new_all();
    system.refresh_cpu_all();

    let cpu_brand = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(HostInfo {
        hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
        os_name: System::long_os_version()
            .or_else(System::name)
            .unwrap_or_else(|| "unknown".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "unknown".to_string()),
        kernel_version: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        arch: System::cpu_arch(),
        cpu_brand,
        physical_cores: system.physical_core_count(),
        logical_cores: system.cpus().len(),
        total_memory_bytes: system.total_memory(),
    })
}

/// Every mounted filesystem that looks like real storage.
///
/// Every volume comes back as [`VolumeRole::Data`] or [`VolumeRole::Removable`];
/// identifying which one holds the running OS is platform-specific, so the
/// caller applies that.
pub fn raw_volumes() -> Result<Vec<Volume>> {
    let disks = Disks::new_with_refreshed_list();
    let mut volumes = Vec::new();

    for disk in &disks {
        let filesystem = disk.file_system().to_string_lossy().to_string();
        if is_pseudo_filesystem(&filesystem) {
            continue;
        }
        // A zero-sized volume is an unmounted or virtual device. There is
        // nothing meaningful to say about its free space.
        if disk.total_space() == 0 {
            continue;
        }

        volumes.push(Volume {
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            device: disk.name().to_string_lossy().to_string(),
            filesystem,
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
            role: if disk.is_removable() {
                VolumeRole::Removable
            } else {
                VolumeRole::Data
            },
            read_only: disk.is_read_only(),
        });
    }

    volumes.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    Ok(volumes)
}

/// Every running process.
pub fn processes() -> Result<Vec<ProcessInfo>> {
    let mut system = System::new_all();
    // sysinfo derives CPU percentage from the delta between two samples, so a
    // single refresh reports zero for everything. Sampling twice costs one
    // short sleep and is the difference between a useful number and a
    // meaningless one.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_all();

    let mut processes: Vec<ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().to_string(),
            executable: process.exe().map(|path| path.to_string_lossy().to_string()),
            memory_bytes: process.memory(),
            cpu_percent: process.cpu_usage(),
            run_time_secs: process.run_time(),
        })
        .collect();

    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

/// Whether an executable named `tool` can be found on `PATH`.
///
/// This is how probes decide to skip rather than fail. On Windows the
/// `PATHEXT` extensions are tried, so `smartctl` finds `smartctl.exe`.
pub fn tool_on_path(tool: &str) -> bool {
    // An explicit path was given rather than a bare name; just test it.
    if tool.chars().any(std::path::is_separator) {
        return Path::new(tool).is_file();
    }

    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };

    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    };

    std::env::split_paths(&path).any(|dir| {
        if dir.as_os_str().is_empty() {
            return false;
        }
        let base = dir.join(tool);
        if base.is_file() {
            return true;
        }
        extensions
            .iter()
            .any(|ext| base.with_extension(ext.trim_start_matches('.')).is_file())
    })
}

/// Current memory and swap pressure.
pub fn memory_info() -> Result<MemoryInfo> {
    let mut system = System::new();
    system.refresh_memory();
    Ok(MemoryInfo {
        total_bytes: system.total_memory(),
        available_bytes: system.available_memory(),
        swap_total_bytes: system.total_swap(),
        swap_used_bytes: system.used_swap(),
    })
}

/// Output of an external command we shelled out to.
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Run an external tool and capture what it said.
///
/// Every command the tool runs goes through here, so there is one place to
/// hang audit logging off once the fix layer exists. Output is decoded lossily
/// on purpose: a log message containing one malformed byte should not cost us
/// the whole diagnostic.
pub fn run_capture(program: &str, args: &[&str]) -> Result<CommandOutput> {
    use std::process::Command;

    tracing::debug!(program, ?args, "running external command");
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("could not run `{program}`"))?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_filesystems_are_recognised_case_insensitively() {
        assert!(is_pseudo_filesystem("tmpfs"));
        assert!(is_pseudo_filesystem("TmpFS"));
        assert!(!is_pseudo_filesystem("ext4"));
        assert!(!is_pseudo_filesystem("NTFS"));
    }

    #[test]
    fn missing_tools_are_reported_missing() {
        assert!(!tool_on_path("ork-definitely-not-a-real-tool"));
    }
}
