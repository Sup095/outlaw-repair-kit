//! Implementations shared by every platform.
//!
//! Anything in here is genuinely OS-independent -- either pure logic, or a
//! cross-platform crate that already does the OS-specific work correctly. The
//! platform implementations delegate here and then apply their own
//! corrections, which keeps the abstraction seam intact without duplicating
//! work that a maintained crate already does well.

use std::path::Path;

use sysinfo::{Disks, ProcessStatus, System};

use anyhow::Context;

use crate::Result;
use crate::platform::{
    GpuInfo, HostInfo, MemoryInfo, ProcessInfo, ProcessState, Volume, VolumeRole,
};

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
        physical_cores: sysinfo::System::physical_core_count(),
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

    // Who we are, so every process can be compared against it. Read once
    // rather than per process, and allowed to be unknown -- in which case
    // nothing below claims to know whose anything is.
    let ours = sysinfo::get_current_pid()
        .ok()
        .and_then(|pid| system.process(pid))
        .and_then(|process| process.user_id())
        .cloned();

    let mut processes: Vec<ProcessInfo> = system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.as_u32(),
            parent_pid: process.parent().map(|parent| parent.as_u32()),
            name: process.name().to_string_lossy().to_string(),
            executable: process.exe().map(|path| path.to_string_lossy().to_string()),
            memory_bytes: process.memory(),
            cpu_percent: process.cpu_usage(),
            run_time_secs: process.run_time(),
            // Three answers, not two. An owner that cannot be read is
            // reported as unknown rather than as somebody else's, because
            // the two are different facts and the code that acts on this
            // must be able to tell them apart.
            runs_as_you: match (&ours, process.user_id()) {
                (Some(ours), Some(theirs)) => Some(ours == theirs),
                _ => None,
            },
            state: match process.status() {
                ProcessStatus::Run => ProcessState::Running,
                ProcessStatus::Sleep | ProcessStatus::Idle => ProcessState::Sleeping,
                ProcessStatus::Zombie => ProcessState::Zombie,
                ProcessStatus::Stop => ProcessState::Stopped,
                _ => ProcessState::Other,
            },
        })
        .collect();

    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

/// Locate an executable named `tool` on `PATH`.
///
/// On Windows the `PATHEXT` extensions are tried, so `smartctl` finds
/// `smartctl.exe`.
pub fn which(tool: &str) -> Option<std::path::PathBuf> {
    // An explicit path was given rather than a bare name; just test it.
    if tool.chars().any(std::path::is_separator) {
        let path = Path::new(tool);
        return path.is_file().then(|| path.to_path_buf());
    }

    let path = std::env::var_os("PATH")?;

    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    };

    std::env::split_paths(&path).find_map(|dir| {
        if dir.as_os_str().is_empty() {
            return None;
        }
        let base = dir.join(tool);
        if base.is_file() {
            return Some(base);
        }
        extensions
            .iter()
            .map(|ext| base.with_extension(ext))
            .find(|path| path.is_file())
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
///
/// This waits for the command to finish. For anything that might not finish,
/// use [`crate::exec::run_supervised`], which watches for liveness instead.
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

/// Ask NVIDIA's own tool what its cards have.
///
/// `nvidia-smi` ships with every NVIDIA driver on both supported platforms and
/// reports exactly what we need, which makes it far more trustworthy than
/// inferring video memory from a generic device inventory.
fn nvidia_gpus() -> Vec<GpuInfo> {
    let Some(_) = which("nvidia-smi") else {
        return Vec::new();
    };
    let Ok(output) = run_capture(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,memory.used,driver_version",
            "--format=csv,noheader,nounits",
        ],
    ) else {
        return Vec::new();
    };
    if !output.success {
        return Vec::new();
    }

    output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            // Anything shorter is a format we do not recognise, and guessing
            // which field is which would produce confidently wrong numbers.
            if fields.len() < 4 {
                return None;
            }
            // nvidia-smi reports mebibytes when asked for no units.
            let mib = |value: &str| value.parse::<u64>().ok().map(|mib| mib * 1024 * 1024);
            Some(GpuInfo {
                name: fields[0].to_string(),
                vram_total_bytes: mib(fields[1]),
                vram_used_bytes: mib(fields[2]),
                driver_version: Some(fields[3].to_string()).filter(|v| !v.is_empty()),
            })
        })
        .collect()
}

/// Every graphics processor this machine has, as far as we can tell honestly.
pub fn detect_gpus() -> Vec<GpuInfo> {
    let gpus = nvidia_gpus();
    if !gpus.is_empty() {
        return gpus;
    }
    tracing::debug!("no GPU vendor tooling found; video memory is unknown");
    Vec::new()
}

/// Hand a link to whatever the user normally opens links with.
///
/// Spawned and let go rather than waited on: a browser started from here can
/// live for hours, and the tool has no business holding on to it. Failing to
/// open is not fatal anywhere it is used -- the link is always printed as
/// well, so the worst case is somebody copying it themselves.
///
/// Only `http` and `https` links are accepted. Anything else could name a
/// local program or a protocol handler, and "open whatever this text says" is
/// not a thing a diagnostic tool should be able to do.
pub fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        anyhow::bail!("refusing to open {url}: only http and https links are opened");
    }

    #[cfg(windows)]
    let mut command = {
        // Not `cmd /c start`: `cmd` treats `&` in a URL as a command
        // separator, which silently cuts a query string in half and would run
        // whatever followed it.
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("could not open a browser: {error}"))
}

/// Whether this process is running with administrator or root rights.
///
/// Asked so that a probe needing elevation can be *skipped with a reason*
/// rather than run and fail halfway. Getting this wrong in the optimistic
/// direction is the worse mistake -- a check that claims it can read your
/// drives and then cannot -- so every way of failing to find out answers
/// "no".
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        // The canonical question, asked the way Windows itself answers it.
        // One process spawn per scan, the same cost as any other check here.
        let script = "([Security.Principal.WindowsPrincipal]              [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(              [Security.Principal.WindowsBuiltInRole]::Administrator)";
        run_capture(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", script],
        )
        .map(|output| output.stdout.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    }

    #[cfg(target_os = "linux")]
    {
        // Read rather than spawn: this runs on the way into every scan, and
        // `/proc/self/status` is already in memory.
        //
        // The *effective* uid, not the real one. A program running under sudo
        // has an effective uid of zero and a real uid of whoever invoked it,
        // and it is the effective one that decides what it may open.
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status.lines().find_map(|line| {
                    let rest = line.strip_prefix("Uid:")?;
                    // real, effective, saved, filesystem
                    rest.split_whitespace().nth(1).map(str::to_string)
                })
            })
            .is_some_and(|effective| effective == "0")
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    {
        run_capture("id", &["-u"])
            .map(|output| output.stdout.trim() == "0")
            .unwrap_or(false)
    }

    #[cfg(not(any(windows, unix)))]
    {
        false
    }
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
    fn only_web_links_are_ever_opened() {
        // "Open whatever this text says" would let a crafted link name a
        // local program or a protocol handler.
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "steam://run/570",
            "notepad.exe",
            "",
        ] {
            assert!(open_url(url).is_err(), "{url} was accepted");
        }
    }

    /// A second, independent way of asking the same question.
    ///
    /// `None` when it could not be asked at all, which is not a failure --
    /// only a reason this test cannot judge.
    fn elevated_according_to_something_else() -> Option<bool> {
        #[cfg(windows)]
        {
            // The high-integrity group is present in an elevated process's
            // token and absent otherwise. `whoami` itself needs no rights.
            let output = run_capture("whoami", &["/groups"]).ok()?;
            Some(output.stdout.contains("S-1-16-12288"))
        }
        #[cfg(unix)]
        {
            let output = run_capture("id", &["-u"]).ok()?;
            Some(output.stdout.trim() == "0")
        }
        #[cfg(not(any(windows, unix)))]
        {
            None
        }
    }

    #[test]
    fn elevation_is_reported_the_same_way_something_else_reports_it() {
        // Deliberately not an assertion about *which* answer: CI runners are
        // elevated on Windows and are not on Linux, and a test that expects
        // one of those is testing the runner rather than the code. What
        // matters is that this agrees with an independent way of asking --
        // getting it wrong in the optimistic direction means telling somebody
        // a check needs rights they had already given it.
        let ours = is_elevated();
        let Some(theirs) = elevated_according_to_something_else() else {
            return;
        };
        assert_eq!(
            ours, theirs,
            "is_elevated() said {ours}, but asking another way said {theirs}"
        );
    }

    #[test]
    fn asking_twice_gives_the_same_answer() {
        // This runs on the way into every scan, so it must be cheap and it
        // must not wander.
        assert_eq!(is_elevated(), is_elevated());
    }

    #[test]
    fn missing_tools_are_reported_missing() {
        assert!(which("ork-definitely-not-a-real-tool").is_none());
    }

    #[test]
    fn a_tool_that_exists_resolves_to_a_real_file() {
        // Every supported platform has a shell interpreter on PATH under one
        // of these names.
        let found = which(if cfg!(windows) { "cmd" } else { "sh" });
        let found = found.expect("expected to find a shell on PATH");
        assert!(
            found.is_file(),
            "which returned {found:?}, which is not a file"
        );
    }
}
