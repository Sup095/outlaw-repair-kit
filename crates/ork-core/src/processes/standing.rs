//! Deciding what may be stopped, and what may never be.
//!
//! This is the safety-critical half of the process work, and it is deliberately
//! the first half to exist. It stops nothing. It only answers, for one running
//! process, which of three standings it has: never touched, not offered unless
//! you insist, or offered.
//!
//! The asymmetry matters more than the accuracy. Wrongly protecting something
//! costs a few megabytes that could have been freed. Wrongly offering something
//! costs somebody their audio, their security software, or their unsaved work.
//! So every rule here errs toward protection, and where two rules disagree the
//! more protective one wins.
//!
//! **What this is not.** Matching on the name of a program is not
//! identification. Something malicious can call itself whatever it likes, and
//! naming itself after a driver would get it protected here. That is the safe
//! direction to be wrong in for a list whose job is to stop *this tool* from
//! breaking a machine, and it is the wrong direction for a list that claimed to
//! find malware -- which is why this one does not claim to.

use crate::platform::{PlatformKind, ProcessInfo};
use crate::processes::in_front::InFront;

/// What may be done with a process.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "standing", rename_all = "kebab-case")]
pub enum Standing {
    /// Never stopped, whatever anybody asks for. There is no flag for this.
    Protected { because: Protection },
    /// Not offered by default. It can be chosen deliberately, one at a time,
    /// and it is never ticked for you.
    HeldBack { because: Restraint },
    /// Offered, and ticked by default.
    Candidate,
}

impl Standing {
    /// Whether a sweep would stop this without anybody singling it out.
    pub fn stopped_by_default(&self) -> bool {
        matches!(self, Standing::Candidate)
    }

    /// Whether this can be stopped at all, however deliberately.
    pub fn can_ever_be_stopped(&self) -> bool {
        !matches!(self, Standing::Protected { .. })
    }
}

/// Why something is never touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protection {
    /// The operating system, the session, and the desktop hangs off it.
    OperatingSystem,
    /// Antivirus and endpoint software. Stopping one is indistinguishable from
    /// what malware does, and would get this tool flagged, quite rightly.
    Security,
    /// A driver, or the control panel that belongs to one -- graphics, audio,
    /// chipset, input. Stopping the panel that owns your audio to save forty
    /// megabytes is a bad trade every time.
    DriverOrControlPanel,
    /// The display, input, and audio stack itself.
    DisplayInputAudio,
    /// Networking, and anything holding the machine's connections open.
    Networking,
    /// Disk encryption, and anything holding a volume open.
    DiskEncryption,
    /// A screen reader or magnifier. Stopping one locks somebody out of their
    /// own computer, with no way for them to undo it.
    Accessibility,
    /// This tool, and the terminal or window it is running in.
    TheToolItself,
}

impl Protection {
    /// Said to a person, in a list of what was left alone.
    pub fn describe(self) -> &'static str {
        match self {
            Protection::OperatingSystem => "part of the operating system",
            Protection::Security => "security software",
            Protection::DriverOrControlPanel => "a driver or its control panel",
            Protection::DisplayInputAudio => "part of the display, input, or audio stack",
            Protection::Networking => "part of networking",
            Protection::DiskEncryption => "disk encryption",
            Protection::Accessibility => "accessibility software",
            Protection::TheToolItself => "this tool",
        }
    }
}

/// Why something is not offered by default, though it could be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Restraint {
    /// It belongs to another account -- SYSTEM, root, a service account, or
    /// another person logged in. Reaching it needs rights this does not have
    /// by default, and things running there are usually there for a reason.
    ///
    /// Also the answer when the owner could not be read at all, which on
    /// Windows is what an unprivileged process gets when it asks about a
    /// service. Not knowing whose something is is not the same as knowing it
    /// is yours.
    RunsAsAnotherAccount,
    /// It has a window in front of you right now.
    InFrontOfYou,
    /// Started in the last few minutes, so you probably started it.
    JustStarted,
    /// A browser, a chat client, anything with a text box in it. Never ticked
    /// for you; ticking it yourself is a decision about your own work.
    MayHoldUnsavedWork,
    /// The tool knows how to stop it and not how to start it again. Saying so
    /// before the button is pressed is the difference between a feature and a
    /// trap.
    CannotBeRestarted,
    /// A file-syncing agent. Idle it is fair game; mid-transfer it is not, and
    /// from outside there is no telling which, so it is never ticked for you.
    MayBeSyncingFiles,
    /// Part of another program rather than a program -- an embedded browser,
    /// a helper the parent will not survive losing. Stopping it breaks
    /// whatever it belongs to, which is not what anybody asked for.
    BelongsToAnotherProgram,
    /// The thing somebody would reach for to undo this.
    HowYouWouldRecover,
    /// You said to leave this one alone.
    Pinned,
}

impl Restraint {
    pub fn describe(self) -> &'static str {
        match self {
            Restraint::RunsAsAnotherAccount => "not yours -- it runs as another account",
            Restraint::InFrontOfYou => "in front of you right now",
            Restraint::JustStarted => "started just now",
            Restraint::MayHoldUnsavedWork => "may have unsaved work in it",
            Restraint::CannotBeRestarted => "this tool could not start it again",
            Restraint::MayBeSyncingFiles => "may be part-way through syncing files",
            Restraint::BelongsToAnotherProgram => "belongs to another program",
            Restraint::HowYouWouldRecover => "what you would use to undo this",
            Restraint::Pinned => "you asked to leave this one alone",
        }
    }
}

/// What the classifier knows beyond the process itself.
#[derive(Debug, Clone, Default)]
pub struct Circumstances {
    /// Names the person has asked to be left alone, lower-case.
    pub pinned: Vec<String>,
    /// What has the window in front of the user, or why that could not be
    /// established. See [`crate::processes::in_front`] -- and note that not
    /// knowing is not the careful answer here: it means this rail protects
    /// nothing, which anything showing a sweep is expected to say out loud.
    pub in_front: InFront,
    /// The process in front of you and everything it is running inside.
    ///
    /// Ancestors as well as the process itself, because stopping what
    /// launched the thing you are looking at takes the thing you are looking
    /// at with it. A game started from Steam is the ordinary case: Steam is
    /// idle, holds several hundred megabytes, and looks like an excellent
    /// candidate right up until stopping it takes the game down.
    pub in_front_lineage: Vec<u32>,
    /// Names of the programs in front of you, lower-case.
    ///
    /// The same reasoning as [`Circumstances::own_family`], for the same
    /// reason: a modern application is not one process. A browser is forty
    /// processes sharing a name, and stopping thirty-nine of them while
    /// somebody looks at the fortieth is stopping the one they are looking
    /// at.
    pub in_front_family: Vec<String>,
    /// This process and every process it is running inside: the shell, the
    /// terminal, and whatever launched that.
    ///
    /// A list rather than one number, because protecting only our own process
    /// is not enough. Run from a terminal, the tool would happily class the
    /// terminal as a candidate and stop it -- taking itself with it, halfway
    /// through a sweep, with the restore file written and nothing left running
    /// to read it. Built with [`lineage_of`].
    pub own_lineage: Vec<u32>,
    /// Names of the programs the tool is running inside, lower-case.
    ///
    /// Separate from the lineage because a modern application is not one
    /// process. Run inside one, the ancestor walk protects the handful of
    /// processes directly above the tool and leaves the other twenty belonging
    /// to the same application looking like ordinary candidates -- which is
    /// exactly what happened the first time this was pointed at a real
    /// machine. Stopping those kills the application the tool is running in
    /// just as dead. Built with [`family_of`].
    pub own_family: Vec<String>,
}

/// The distinct program names in a lineage.
///
/// Everything sharing a name with something the tool is running inside is
/// treated as part of the same application and left alone.
pub fn family_of(lineage: &[u32], processes: &[ProcessInfo]) -> Vec<String> {
    let mut names: Vec<String> = lineage
        .iter()
        .filter_map(|pid| processes.iter().find(|p| p.pid == *pid))
        .map(leaf_name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A process, its parent, its parent's parent, and so on to the top.
///
/// Used to protect everything the tool is running inside. Walks by parent
/// identifier and stops if the chain loops or runs away, because a process
/// table read a moment ago is a snapshot and the identifiers in it may already
/// have been reused.
pub fn lineage_of(pid: u32, processes: &[ProcessInfo]) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    for _ in 0..64 {
        let Some(process) = processes.iter().find(|p| p.pid == current) else {
            break;
        };
        let Some(parent) = process.parent_pid else {
            break;
        };
        if parent == 0 || chain.contains(&parent) {
            break;
        }
        chain.push(parent);
        current = parent;
    }
    chain
}

/// Anything younger than this was probably started by the person, just now.
pub const JUST_STARTED_SECS: u64 = 5 * 60;

/// Decide the standing of one process.
///
/// Order matters, and it is protection first. A process that is both security
/// software and in front of you is protected, not held back -- the more
/// protective answer always wins.
pub fn classify(process: &ProcessInfo, platform: PlatformKind, about: &Circumstances) -> Standing {
    if about.own_lineage.contains(&process.pid) {
        return Standing::Protected {
            because: Protection::TheToolItself,
        };
    }

    let name = leaf_name(process);
    if about.own_family.iter().any(|member| member == &name) {
        return Standing::Protected {
            because: Protection::TheToolItself,
        };
    }
    if let Some(protection) = protection_for(&name, platform) {
        return Standing::Protected {
            because: protection,
        };
    }

    // Held-back reasons, most serious first, so the reason shown is the one
    // that would matter most to somebody deciding.
    if about.pinned.iter().any(|pinned| pinned == &name) {
        return Standing::HeldBack {
            because: Restraint::Pinned,
        };
    }
    if HOLDS_WORK.contains(&name.as_str()) {
        return Standing::HeldBack {
            because: Restraint::MayHoldUnsavedWork,
        };
    }
    if SYNCS_FILES.contains(&name.as_str()) {
        return Standing::HeldBack {
            because: Restraint::MayBeSyncingFiles,
        };
    }
    if PART_OF_SOMETHING_ELSE.contains(&name.as_str()) {
        return Standing::HeldBack {
            because: Restraint::BelongsToAnotherProgram,
        };
    }
    if RECOVERY_TOOLS.contains(&name.as_str()) {
        return Standing::HeldBack {
            because: Restraint::HowYouWouldRecover,
        };
    }
    // Read from the process rather than from the circumstances, because whose
    // a process is is a fact about that process. It was on the circumstances
    // once, which meant one answer for the whole machine: either every service
    // was held back or none was, and neither is a list anybody could trust.
    //
    // `None` means the question could not be answered, and an unanswered
    // question about who owns something is treated as the careful answer.
    // Before the ownership question, because when both are true this is the
    // reason worth showing: "you are looking at it" is something somebody can
    // check for themselves in a second, and "it runs as another account" is
    // not.
    if about.in_front_lineage.contains(&process.pid)
        || about.in_front_family.iter().any(|member| member == &name)
    {
        return Standing::HeldBack {
            because: Restraint::InFrontOfYou,
        };
    }
    if process.runs_as_you != Some(true) {
        return Standing::HeldBack {
            because: Restraint::RunsAsAnotherAccount,
        };
    }
    if process.run_time_secs < JUST_STARTED_SECS {
        return Standing::HeldBack {
            because: Restraint::JustStarted,
        };
    }

    Standing::Candidate
}

/// The executable's own name, lower-cased, without any directory.
///
/// Taken from the executable path where there is one, because that is harder to
/// disguise than the reported name, and falling back to the name when there is
/// not.
///
/// Both separators, always, rather than `Path::file_name`. That asks the
/// machine this code is *running* on what a separator is, and the path being
/// examined does not necessarily come from that machine: a Windows path read on
/// Linux has no separators at all as far as `Path` is concerned, so
/// `C:\Windows\System32\svchost.exe` comes back whole and matches nothing in
/// any list. The tool can already look at a scan from a paired machine, so the
/// platform being classified and the platform doing the classifying are two
/// different questions -- and the one that decides this is the platform
/// argument, not the build.
fn leaf_name(process: &ProcessInfo) -> String {
    let from_path = process.executable.as_deref().and_then(|path| {
        path.rsplit(['/', '\\'])
            .next()
            .filter(|leaf| !leaf.is_empty())
            .map(str::to_ascii_lowercase)
    });
    from_path.unwrap_or_else(|| process.name.to_ascii_lowercase())
}

/// Whether this name is protected, and why.
fn protection_for(name: &str, platform: PlatformKind) -> Option<Protection> {
    let shared = [
        (OURS, Protection::TheToolItself),
        (ACCESSIBILITY, Protection::Accessibility),
        (SECURITY, Protection::Security),
    ];
    for (list, protection) in shared {
        if list.contains(&name) {
            return Some(protection);
        }
    }

    let per_platform: &[(&[&str], Protection)] = match platform {
        PlatformKind::Windows => &[
            (WINDOWS_OS, Protection::OperatingSystem),
            (WINDOWS_DRIVERS, Protection::DriverOrControlPanel),
            (WINDOWS_DISPLAY, Protection::DisplayInputAudio),
            (WINDOWS_NETWORK, Protection::Networking),
            (WINDOWS_ENCRYPTION, Protection::DiskEncryption),
        ],
        PlatformKind::Linux => &[
            (LINUX_OS, Protection::OperatingSystem),
            (LINUX_DRIVERS, Protection::DriverOrControlPanel),
            (LINUX_DISPLAY, Protection::DisplayInputAudio),
            (LINUX_NETWORK, Protection::Networking),
            (LINUX_ENCRYPTION, Protection::DiskEncryption),
        ],
        PlatformKind::MacOs => &[],
    };
    for (list, protection) in per_platform {
        if list.contains(&name) {
            return Some(*protection);
        }
    }
    None
}

/// This tool, under every name it goes by.
const OURS: &[&str] = &["outlaw", "outlaw.exe", "ork-desktop", "ork-desktop.exe"];

/// Accessibility software. Stopping one of these locks somebody out of the
/// machine they would need in order to put it back.
const ACCESSIBILITY: &[&str] = &[
    "narrator.exe",
    "magnify.exe",
    "osk.exe",
    "nvda.exe",
    "jfw.exe",
    "dragonbar.exe",
    "orca",
    "at-spi2-registryd",
    "at-spi-bus-launcher",
    "speech-dispatcher",
    "espeak-ng",
];

/// Antivirus and endpoint agents, across the common vendors. Generous on
/// purpose: a false match here costs nothing at all.
const SECURITY: &[&str] = &[
    "msmpeng.exe",
    "nissrv.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "mpdefendercoreservice.exe",
    "smartscreen.exe",
    "csfalconservice.exe",
    "csfalconcontainer.exe",
    "sentinelagent.exe",
    "sentinelstaticengine.exe",
    "cylancesvc.exe",
    "cbdefense.exe",
    "xagt.exe",
    "avp.exe",
    "avgsvc.exe",
    "avastsvc.exe",
    "bdagent.exe",
    "vsserv.exe",
    "ekrn.exe",
    "egui.exe",
    "mbamservice.exe",
    "mbam.exe",
    "malwarebytes.exe",
    "mbamtray.exe",
    "sechealthui.exe",
    "msascuil.exe",
    "msseces.exe",
    "windowsdefender.exe",
    "securityhealthhost.exe",
    "sophosui.exe",
    "savservice.exe",
    "mcshield.exe",
    "nortonsecurity.exe",
    "ns.exe",
    "clamd",
    "freshclam",
    "falcon-sensor",
    "sentinelone",
    "wdavdaemon",
    "auditd",
];

const WINDOWS_OS: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "sihost.exe",
    "taskhostw.exe",
    "explorer.exe",
    "runtimebroker.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "searchindexer.exe",
    "ctfmon.exe",
    "dllhost.exe",
    "conhost.exe",
    "wudfhost.exe",
    "applicationframehost.exe",
    "lockapp.exe",
    "searchapp.exe",
    "systemsettings.exe",
    "useroobebroker.exe",
    "wmiapsrv.exe",
    "[system process]",
    "dashost.exe",
    "memory compression",
    "lsaiso.exe",
    "wmiprvse.exe",
    "spoolsv.exe",
    "trustedinstaller.exe",
    "tiworker.exe",
];

/// Drivers and the control panels that belong to them.
const WINDOWS_DRIVERS: &[&str] = &[
    "nvcontainer.exe",
    "nvdisplay.container.exe",
    "nvidia share.exe",
    "nvidia web helper.exe",
    "nvsphelper64.exe",
    "amddvr.exe",
    "radeonsoftware.exe",
    "amdrsserv.exe",
    "atieclxx.exe",
    "atiesrxx.exe",
    "igfxem.exe",
    "igfxcuiservice.exe",
    "igfxext.exe",
    "rtkauduservice64.exe",
    "ravcpl64.exe",
    "ravbg64.exe",
    "realtekaudiouniversalservice.exe",
    "rtkngui64.exe",
    "nahimicservice.exe",
    "waves.exe",
    "wavessvc64.exe",
    "sscsvc.exe",
    "asusoptimization.exe",
    "asussoftwaremanager.exe",
    "armourycrate.service.exe",
    "logioptionsplus_agent.exe",
    "lghub_agent.exe",
    "corsairservice.exe",
    "icue.exe",
    "razer synapse service.exe",
    "rzsdkservice.exe",
    "steelseriesengine.exe",
    "elan service.exe",
    "synaptics.exe",
    "syntpenh.exe",
    "fxsound.exe",
    "sonicstudio.exe",
    "dtsapo4service.exe",
];

const WINDOWS_DISPLAY: &[&str] = &[
    "dwm.exe",
    "audiodg.exe",
    "audioendpointbuilder.exe",
    "textinputhost.exe",
    "logonui.exe",
];

const WINDOWS_NETWORK: &[&str] = &[
    "dnscache.exe",
    "netprofm.exe",
    "wlanext.exe",
    "vpnagent.exe",
    "openvpn.exe",
    "wireguard.exe",
    "tailscaled.exe",
    "tailscale-ipn.exe",
];

const WINDOWS_ENCRYPTION: &[&str] = &[
    "bdesvc.exe",
    "veracrypt.exe",
    "bitlockerdeviceencryption.exe",
];

const LINUX_OS: &[&str] = &[
    "systemd",
    "systemd-journald",
    "systemd-udevd",
    "systemd-logind",
    "systemd-resolved",
    "systemd-oomd",
    "init",
    "kthreadd",
    "dbus-daemon",
    "dbus-broker",
    "polkitd",
    "udisksd",
    "accounts-daemon",
    "gdm",
    "gdm-session-worker",
    "sddm",
    "lightdm",
    "gnome-shell",
    "gnome-session-binary",
    "plasmashell",
    "plasma_session",
    "kwin_x11",
    "kwin_wayland",
    "mutter",
    "xfwm4",
    "xfce4-session",
    "cinnamon",
    "sshd",
    "cron",
    "crond",
    "rsyslogd",
    "dbus-run-session",
];

const LINUX_DRIVERS: &[&str] = &[
    "nvidia-persistenced",
    "nvidia-settings",
    "nvidia-powerd",
    "amdgpu",
    "irqbalance",
    "thermald",
    "upowerd",
    "bluetoothd",
    "fwupd",
];

const LINUX_DISPLAY: &[&str] = &[
    "xorg",
    "x",
    "wayland",
    "pipewire",
    "pipewire-pulse",
    "wireplumber",
    "pulseaudio",
    "jackd",
    "xdg-desktop-portal",
    "xdg-desktop-portal-gnome",
    "xdg-desktop-portal-kde",
    "ibus-daemon",
    "fcitx5",
];

const LINUX_NETWORK: &[&str] = &[
    "networkmanager",
    "nm-applet",
    "wpa_supplicant",
    "dhcpcd",
    "dhclient",
    "connmand",
    "iwd",
    "tailscaled",
    "openvpn",
    "wg-quick",
];

const LINUX_ENCRYPTION: &[&str] = &["cryptsetup", "systemd-cryptsetup", "veracrypt"];

/// File-syncing agents.
///
/// Idle, one of these is a fair candidate. Part-way through uploading a file
/// it is not, and from outside there is no way to tell which -- so it is never
/// ticked for somebody, and they can tick it when they know.
const SYNCS_FILES: &[&str] = &[
    "onedrive.exe",
    "onedrive.sync.service.exe",
    "filecoauth.exe",
    "dropbox.exe",
    "dropbox",
    "googledrivefs.exe",
    "googledrivesync.exe",
    "megasync.exe",
    "nextcloud.exe",
    "nextcloud",
    "syncthing.exe",
    "syncthing",
    "insync.exe",
    "pcloud.exe",
    "backblaze.exe",
    "bzbui.exe",
    "resilio sync.exe",
];

/// Pieces of other programs rather than programs.
///
/// An embedded browser belongs to whatever is hosting it. Stopping one of
/// these frees memory by breaking an application somebody is using, which is
/// not what "stop what I am not using" meant.
const PART_OF_SOMETHING_ELSE: &[&str] = &[
    "msedgewebview2.exe",
    "webview2",
    "qtwebengineprocess.exe",
    "qtwebengineprocess",
    "electron.exe",
    "crashpad_handler.exe",
    "crashpad_handler",
];

/// What somebody would reach for to undo any of this.
///
/// Freeing sixty megabytes by closing the window somebody would use to see
/// what happened is a poor trade, and an alarming one to watch.
const RECOVERY_TOOLS: &[&str] = &[
    "taskmgr.exe",
    "resmon.exe",
    "perfmon.exe",
    "procexp.exe",
    "procexp64.exe",
    "processlasso.exe",
    "bitsumsessionagent.exe",
    "procmon.exe",
    "procmon64.exe",
    "mmc.exe",
    "regedit.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "conhost.exe",
    "htop",
    "top",
    "btop",
    "gnome-system-monitor",
    "ksysguard",
    "plasma-systemmonitor",
    "xterm",
    "konsole",
    "gnome-terminal-server",
    "alacritty",
    "kitty",
    "wezterm-gui",
];

/// Programs likely to have something unsaved in them.
///
/// Never ticked for you. Deliberately generous, and deliberately not a list of
/// editors -- an editor is not offered at all, which is a different rule
/// enforced by the sweep rather than here.
const HOLDS_WORK: &[&str] = &[
    "firefox.exe",
    "firefox",
    "chrome.exe",
    "chrome",
    "msedge.exe",
    "brave.exe",
    "brave",
    "opera.exe",
    "vivaldi.exe",
    "librewolf.exe",
    "zen.exe",
    "discord.exe",
    "discord",
    "slack.exe",
    "slack",
    "teams.exe",
    "ms-teams.exe",
    "telegram.exe",
    "telegram-desktop",
    "signal.exe",
    "signal-desktop",
    "whatsapp.exe",
    "element.exe",
    "element-desktop",
    "thunderbird.exe",
    "thunderbird",
    "obsidian.exe",
    "obsidian",
    "notion.exe",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ProcessState;

    fn process(name: &str, pid: u32) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent_pid: None,
            name: name.to_string(),
            executable: Some(format!("C:\\Program Files\\Thing\\{name}")),
            memory_bytes: 100 * 1024 * 1024,
            cpu_percent: 0.0,
            // Old enough not to trip the just-started rule.
            run_time_secs: 60 * 60,
            runs_as_you: Some(true),
            state: ProcessState::Running,
        }
    }

    /// Circumstances in which an ordinary program would be a candidate, so a
    /// test that gets something else has learned something.
    fn ordinary() -> Circumstances {
        Circumstances {
            pinned: Vec::new(),
            in_front: InFront::Nothing,
            in_front_lineage: Vec::new(),
            in_front_family: Vec::new(),
            own_lineage: vec![999_999],
            own_family: Vec::new(),
        }
    }

    fn standing(name: &str, platform: PlatformKind) -> Standing {
        classify(&process(name, 42), platform, &ordinary())
    }

    #[test]
    fn an_ordinary_background_program_is_a_candidate() {
        // The control. If this ever stops being true, every test below is
        // passing for the wrong reason.
        assert_eq!(
            standing("SomeUpdater.exe", PlatformKind::Windows),
            Standing::Candidate
        );
    }

    #[test]
    fn security_software_is_never_touched() {
        // Suspending an endpoint agent is indistinguishable from what malware
        // does. Getting this wrong would be worse than the feature is worth.
        for name in [
            "MsMpEng.exe",
            "CSFalconService.exe",
            "SentinelAgent.exe",
            "ekrn.exe",
            "wdavdaemon",
        ] {
            let platform = if name.ends_with(".exe") {
                PlatformKind::Windows
            } else {
                PlatformKind::Linux
            };
            assert_eq!(
                standing(name, platform),
                Standing::Protected {
                    because: Protection::Security
                },
                "{name} was not protected"
            );
        }
    }

    #[test]
    fn accessibility_software_is_never_touched() {
        // Somebody who cannot see the screen cannot undo this. There is no
        // amount of memory that makes it acceptable.
        assert_eq!(
            standing("nvda.exe", PlatformKind::Windows),
            Standing::Protected {
                because: Protection::Accessibility
            }
        );
        assert_eq!(
            standing("orca", PlatformKind::Linux),
            Standing::Protected {
                because: Protection::Accessibility
            }
        );
    }

    #[test]
    fn drivers_and_their_control_panels_are_never_touched() {
        for name in [
            "NVDisplay.Container.exe",
            "RtkAudUService64.exe",
            "iCUE.exe",
        ] {
            assert_eq!(
                standing(name, PlatformKind::Windows),
                Standing::Protected {
                    because: Protection::DriverOrControlPanel
                },
                "{name} was not protected"
            );
        }
        assert_eq!(
            standing("nvidia-persistenced", PlatformKind::Linux),
            Standing::Protected {
                because: Protection::DriverOrControlPanel
            }
        );
    }

    #[test]
    fn the_operating_system_is_never_touched() {
        for name in ["csrss.exe", "lsass.exe", "explorer.exe", "svchost.exe"] {
            assert!(
                !standing(name, PlatformKind::Windows).can_ever_be_stopped(),
                "{name} could be stopped"
            );
        }
        for name in ["systemd", "gnome-shell", "dbus-daemon"] {
            assert!(
                !standing(name, PlatformKind::Linux).can_ever_be_stopped(),
                "{name} could be stopped"
            );
        }
    }

    #[test]
    fn the_audio_and_display_stack_is_never_touched() {
        assert!(!standing("audiodg.exe", PlatformKind::Windows).can_ever_be_stopped());
        assert!(!standing("dwm.exe", PlatformKind::Windows).can_ever_be_stopped());
        assert!(!standing("pipewire", PlatformKind::Linux).can_ever_be_stopped());
    }

    #[test]
    fn the_tool_never_sweeps_itself_away() {
        // Two ways round: by name, and by being this very process. The second
        // is what catches a build running under a different name.
        assert_eq!(
            standing("outlaw.exe", PlatformKind::Windows),
            Standing::Protected {
                because: Protection::TheToolItself
            }
        );
        let me = classify(
            &process("anything-at-all.exe", 4242),
            PlatformKind::Windows,
            &Circumstances {
                own_lineage: vec![4242],
                ..ordinary()
            },
        );
        assert_eq!(
            me,
            Standing::Protected {
                because: Protection::TheToolItself
            }
        );
    }

    #[test]
    fn a_program_that_might_hold_unsaved_work_is_never_ticked_for_you() {
        // It can be chosen. It is never chosen on somebody's behalf.
        let it = standing("firefox.exe", PlatformKind::Windows);
        assert_eq!(
            it,
            Standing::HeldBack {
                because: Restraint::MayHoldUnsavedWork
            }
        );
        assert!(!it.stopped_by_default());
        assert!(it.can_ever_be_stopped());
    }

    #[test]
    fn names_are_matched_however_they_are_capitalised() {
        // Windows reports these inconsistently, and a list that only matched
        // one spelling would protect nothing on half the machines it ran on.
        for spelling in ["MsMpEng.exe", "msmpeng.exe", "MSMPENG.EXE"] {
            assert_eq!(
                standing(spelling, PlatformKind::Windows),
                Standing::Protected {
                    because: Protection::Security
                },
                "{spelling} was not protected"
            );
        }
    }

    #[test]
    fn the_name_is_taken_from_the_path_rather_than_the_reported_name() {
        // The reported name is the easier of the two to disguise, and this
        // list exists to stop the tool breaking a machine.
        let mut it = process("something-harmless.exe", 7);
        it.executable = Some("C:\\Windows\\System32\\lsass.exe".to_string());
        assert!(!classify(&it, PlatformKind::Windows, &ordinary()).can_ever_be_stopped());
    }

    #[test]
    fn a_path_is_split_the_way_the_machine_it_came_from_writes_them() {
        // Asking `Path` for the last part of a path asks the machine running
        // this code what a separator is, and the answer is wrong whenever the
        // path came from somewhere else -- which it can, because this tool
        // already reads a scan from a paired machine. A Windows path examined
        // on Linux has no separators at all as far as `Path` is concerned, so
        // the whole string is treated as the name and matches nothing.
        //
        // Both directions are checked, and only one of them can fail on any
        // given machine. That is the point of running the tests on both.
        let mut windows_path = process("disguised", 7);
        windows_path.executable = Some(r"C:\Windows\System32\lsass.exe".to_string());
        assert_eq!(leaf_name(&windows_path), "lsass.exe");

        let mut linux_path = process("disguised", 7);
        linux_path.executable = Some("/usr/lib/systemd/systemd-journald".to_string());
        assert_eq!(leaf_name(&linux_path), "systemd-journald");

        // A path that is nothing but a name, and one that ends in a separator,
        // both of which appear in real process listings.
        let mut bare = process("fallback-name", 7);
        bare.executable = Some("sshd".to_string());
        assert_eq!(leaf_name(&bare), "sshd");

        let mut trailing = process("fallback-name", 7);
        trailing.executable = Some("/usr/bin/".to_string());
        assert_eq!(
            leaf_name(&trailing),
            "fallback-name",
            "a path with nothing after the separator should fall back to the name"
        );
    }

    #[test]
    fn a_process_with_no_path_is_still_judged_by_its_name() {
        // Plenty of processes report no executable at all, especially the ones
        // running with rights this does not have. Falling through to Candidate
        // for those would be exactly the wrong way to fail.
        let mut it = process("MsMpEng.exe", 7);
        it.executable = None;
        assert_eq!(
            classify(&it, PlatformKind::Windows, &ordinary()),
            Standing::Protected {
                because: Protection::Security
            }
        );
    }

    #[test]
    fn not_knowing_who_owns_something_is_the_careful_answer() {
        // `None` is not `false`. Treating an unanswered question as "yes, it
        // is yours" is how a tool ends up stopping something it had no
        // business touching -- and on Windows, `None` is the ordinary answer
        // for every service, which is exactly the set that matters.
        let mut unknown = process("SomeUpdater.exe", 42);
        unknown.runs_as_you = None;
        assert_eq!(
            classify(&unknown, PlatformKind::Windows, &ordinary()),
            Standing::HeldBack {
                because: Restraint::RunsAsAnotherAccount
            }
        );
    }

    #[test]
    fn something_belonging_to_another_account_is_not_swept_up() {
        let mut theirs = process("SomeUpdater.exe", 42);
        theirs.runs_as_you = Some(false);
        assert_eq!(
            classify(&theirs, PlatformKind::Windows, &ordinary()),
            Standing::HeldBack {
                because: Restraint::RunsAsAnotherAccount
            }
        );
    }

    #[test]
    fn who_owns_a_process_is_read_from_that_process_and_not_from_the_machine() {
        // This was once a single field on the circumstances, which meant one
        // answer for every process on the machine: either every service was
        // held back or none was. Neither is a list anybody could act on, and
        // the bug was invisible because both halves looked reasonable on
        // their own.
        let mut yours = process("SomeUpdater.exe", 42);
        yours.runs_as_you = Some(true);
        let mut theirs = process("SomeUpdater.exe", 43);
        theirs.runs_as_you = Some(false);

        assert_eq!(
            classify(&yours, PlatformKind::Windows, &ordinary()),
            Standing::Candidate
        );
        assert_ne!(
            classify(&theirs, PlatformKind::Windows, &ordinary()),
            Standing::Candidate,
            "two processes with the same name and different owners must not \
             get the same answer"
        );
    }

    #[test]
    fn what_you_are_looking_at_is_not_swept_away_underneath_you() {
        let looking = Circumstances {
            in_front: InFront::Process(42),
            in_front_lineage: vec![42],
            ..ordinary()
        };
        assert_eq!(
            classify(
                &process("SomeGame.exe", 42),
                PlatformKind::Windows,
                &looking
            ),
            Standing::HeldBack {
                because: Restraint::InFrontOfYou
            }
        );
    }

    #[test]
    fn what_started_what_you_are_looking_at_goes_with_it() {
        // The ordinary case, and the one this was built for: a game started
        // from Steam. Steam itself is idle, holds several hundred megabytes,
        // and is an excellent candidate right up until stopping it takes the
        // game down with it.
        let looking = Circumstances {
            in_front: InFront::Process(42),
            in_front_lineage: vec![42, 7],
            // Left empty on purpose. `family_of` would fill it from the
            // lineage in real use, so with it filled this test passed with
            // the ancestor check deleted -- which is not a test of the
            // ancestor check.
            in_front_family: Vec::new(),
            ..ordinary()
        };
        assert_eq!(
            classify(&process("steam.exe", 7), PlatformKind::Windows, &looking),
            Standing::HeldBack {
                because: Restraint::InFrontOfYou
            },
            "the launcher the focused game is running inside must not be a              candidate"
        );
    }

    #[test]
    fn the_other_processes_of_what_you_are_looking_at_go_with_it() {
        // A modern application is not one process. A browser is forty of them
        // sharing a name, and only one owns the window; stopping the other
        // thirty-nine is stopping the one being looked at. Same lesson as
        // `own_family`, learned the same way.
        let looking = Circumstances {
            in_front: InFront::Process(42),
            in_front_lineage: vec![42],
            in_front_family: vec!["somegame.exe".to_string()],
            ..ordinary()
        };
        assert_eq!(
            classify(
                &process("SomeGame.exe", 5_000),
                PlatformKind::Windows,
                &looking
            ),
            Standing::HeldBack {
                because: Restraint::InFrontOfYou
            }
        );
    }

    #[test]
    fn being_looked_at_is_the_reason_shown_when_several_apply() {
        // Both true: in front of you and running as somebody else. The reason
        // worth printing is the one somebody can check for themselves in a
        // second by looking at their own screen.
        let mut it = process("SomeGame.exe", 42);
        it.runs_as_you = Some(false);
        let looking = Circumstances {
            in_front: InFront::Process(42),
            in_front_lineage: vec![42],
            ..ordinary()
        };
        assert_eq!(
            classify(&it, PlatformKind::Windows, &looking),
            Standing::HeldBack {
                because: Restraint::InFrontOfYou
            }
        );
    }

    #[test]
    fn not_being_able_to_tell_holds_nothing_back() {
        // Stated as a test because it is the uncomfortable half of this rail
        // and must not be quietly changed. Unknown cannot mean "hold
        // everything back" -- that would make a sweep useless -- so it means
        // this rail protects nothing, and the answer is carried so that
        // whatever shows the sweep can say so.
        let cannot_tell = Circumstances {
            in_front: InFront::Unknown("no desktop session".to_string()),
            ..ordinary()
        };
        assert_eq!(
            classify(
                &process("SomeUpdater.exe", 42),
                PlatformKind::Windows,
                &cannot_tell
            ),
            Standing::Candidate
        );
        assert_eq!(
            cannot_tell.in_front.unanswered(),
            Some("no desktop session"),
            "the reason must survive on the circumstances, or nothing can              report that the rail did not run"
        );
    }

    #[test]
    fn something_started_a_moment_ago_was_probably_started_by_you() {
        let mut just_now = process("SomeUpdater.exe", 42);
        just_now.run_time_secs = 10;
        assert_eq!(
            classify(&just_now, PlatformKind::Windows, &ordinary()),
            Standing::HeldBack {
                because: Restraint::JustStarted
            }
        );
    }

    #[test]
    fn pinning_something_holds_it_back() {
        let pinned = Circumstances {
            pinned: vec!["someupdater.exe".to_string()],
            ..ordinary()
        };
        assert_eq!(
            classify(
                &process("SomeUpdater.exe", 42),
                PlatformKind::Windows,
                &pinned
            ),
            Standing::HeldBack {
                because: Restraint::Pinned
            }
        );
    }

    #[test]
    fn protection_beats_every_other_reason() {
        // Security software that also happens to be in front of you, pinned,
        // and started a moment ago is still protected -- not held back. The
        // more protective answer wins, always, and the reason shown is the one
        // that means "there is no flag for this".
        let mut it = process("MsMpEng.exe", 42);
        it.run_time_secs = 1;
        it.runs_as_you = Some(false);
        let everything = Circumstances {
            pinned: vec!["msmpeng.exe".to_string()],
            in_front: InFront::Process(42),
            in_front_lineage: vec![42],
            in_front_family: vec!["msmpeng.exe".to_string()],
            own_lineage: vec![1],
            own_family: Vec::new(),
        };
        assert_eq!(
            classify(&it, PlatformKind::Windows, &everything),
            Standing::Protected {
                because: Protection::Security
            }
        );
    }

    #[test]
    fn the_terminal_the_tool_is_running_in_is_never_stopped() {
        // Found by running the classifier over a real machine: every process
        // the tool was running inside came back a candidate. Protecting only
        // our own process is not enough -- stopping the terminal takes the
        // tool with it, halfway through a sweep, with the restore file written
        // and nothing left running to read it.
        let shell = ProcessInfo {
            pid: 100,
            parent_pid: Some(50),
            ..process("SomeHost.exe", 100)
        };
        let terminal = ProcessInfo {
            pid: 50,
            parent_pid: None,
            ..process("SomeTerminal.exe", 50)
        };
        let unrelated = process("SomeUpdater.exe", 77);
        let table = vec![shell.clone(), terminal.clone(), unrelated.clone()];

        let about = Circumstances {
            own_lineage: lineage_of(100, &table),
            ..ordinary()
        };
        for held in [&shell, &terminal] {
            assert_eq!(
                classify(held, PlatformKind::Windows, &about),
                Standing::Protected {
                    because: Protection::TheToolItself
                },
                "{} was not protected",
                held.name
            );
        }
        // And it has not simply protected everything.
        assert_eq!(
            classify(&unrelated, PlatformKind::Windows, &about),
            Standing::Candidate
        );
    }

    #[test]
    fn a_lineage_that_loops_does_not_hang() {
        // A process table is a snapshot, and identifiers get reused. Two
        // processes each claiming the other as a parent is nonsense that must
        // not become an infinite walk.
        let table = vec![
            ProcessInfo {
                parent_pid: Some(2),
                ..process("a.exe", 1)
            },
            ProcessInfo {
                parent_pid: Some(1),
                ..process("b.exe", 2)
            },
        ];
        assert_eq!(lineage_of(1, &table), vec![1, 2]);
    }

    #[test]
    fn a_process_missing_from_the_table_still_protects_itself() {
        assert_eq!(lineage_of(42, &[]), vec![42]);
    }

    #[test]
    fn the_security_software_on_a_real_machine_is_protected() {
        // Every one of these was found running on the machine this was written
        // on, and `Malwarebytes.exe` came back a candidate the first time the
        // classifier was pointed at a real process table. It was in the list
        // under two other names and not that one.
        for name in [
            "Malwarebytes.exe",
            "MBAMService.exe",
            "SecHealthUI.exe",
            "MsMpEng.exe",
        ] {
            assert_eq!(
                standing(name, PlatformKind::Windows),
                Standing::Protected {
                    because: Protection::Security
                },
                "{name} was not protected"
            );
        }
    }

    #[test]
    fn a_sync_agent_is_never_ticked_for_you() {
        // Idle it is fair game. Part-way through uploading a file it is not,
        // and there is no telling which from out here.
        assert_eq!(
            standing("OneDrive.exe", PlatformKind::Windows),
            Standing::HeldBack {
                because: Restraint::MayBeSyncingFiles
            }
        );
    }

    #[test]
    fn a_piece_of_another_program_is_not_swept_up_on_its_own() {
        // An embedded browser belongs to whatever is hosting it. Freeing
        // memory by breaking an application somebody is using is not what
        // "stop what I am not using" meant.
        assert_eq!(
            standing("msedgewebview2.exe", PlatformKind::Windows),
            Standing::HeldBack {
                because: Restraint::BelongsToAnotherProgram
            }
        );
    }

    #[test]
    fn what_you_would_use_to_undo_this_is_left_running() {
        for name in ["Taskmgr.exe", "powershell.exe", "WindowsTerminal.exe"] {
            assert_eq!(
                standing(name, PlatformKind::Windows),
                Standing::HeldBack {
                    because: Restraint::HowYouWouldRecover
                },
                "{name} would have been stopped"
            );
        }
        assert_eq!(
            standing("gnome-system-monitor", PlatformKind::Linux),
            Standing::HeldBack {
                because: Restraint::HowYouWouldRecover
            }
        );
    }

    #[test]
    fn nothing_protected_can_be_stopped_however_it_is_asked_for() {
        // The whole point of the two kinds. Held back is a default; protected
        // is a rule.
        let protected = Standing::Protected {
            because: Protection::Security,
        };
        assert!(!protected.can_ever_be_stopped());
        assert!(!protected.stopped_by_default());

        let held = Standing::HeldBack {
            because: Restraint::MayHoldUnsavedWork,
        };
        assert!(held.can_ever_be_stopped());
        assert!(!held.stopped_by_default());
    }

    #[test]
    fn every_protected_and_held_back_reason_can_be_read_aloud() {
        // These end up on a screen listing what was left alone. A reason with
        // no words is a row somebody cannot act on.
        for reason in [
            Protection::OperatingSystem,
            Protection::Security,
            Protection::DriverOrControlPanel,
            Protection::DisplayInputAudio,
            Protection::Networking,
            Protection::DiskEncryption,
            Protection::Accessibility,
            Protection::TheToolItself,
        ] {
            assert!(!reason.describe().is_empty(), "{reason:?} has no words");
        }
        for reason in [
            Restraint::RunsAsAnotherAccount,
            Restraint::InFrontOfYou,
            Restraint::JustStarted,
            Restraint::MayHoldUnsavedWork,
            Restraint::CannotBeRestarted,
            Restraint::MayBeSyncingFiles,
            Restraint::BelongsToAnotherProgram,
            Restraint::HowYouWouldRecover,
            Restraint::Pinned,
        ] {
            assert!(!reason.describe().is_empty(), "{reason:?} has no words");
        }
    }

    #[test]
    fn the_lists_hold_no_duplicates_and_are_all_lower_case() {
        // Matching is done against a lower-cased name, so an entry with a
        // capital in it can never match anything and is a silent hole in the
        // protection.
        let lists: &[(&str, &[&str])] = &[
            ("ours", OURS),
            ("accessibility", ACCESSIBILITY),
            ("security", SECURITY),
            ("windows os", WINDOWS_OS),
            ("windows drivers", WINDOWS_DRIVERS),
            ("windows display", WINDOWS_DISPLAY),
            ("windows network", WINDOWS_NETWORK),
            ("windows encryption", WINDOWS_ENCRYPTION),
            ("linux os", LINUX_OS),
            ("linux drivers", LINUX_DRIVERS),
            ("linux display", LINUX_DISPLAY),
            ("linux network", LINUX_NETWORK),
            ("linux encryption", LINUX_ENCRYPTION),
            ("holds work", HOLDS_WORK),
            ("syncs files", SYNCS_FILES),
            ("part of something else", PART_OF_SOMETHING_ELSE),
            ("recovery tools", RECOVERY_TOOLS),
        ];
        for (label, list) in lists {
            for name in *list {
                assert_eq!(
                    *name,
                    name.to_ascii_lowercase(),
                    "`{name}` in the {label} list is not lower-case, so it can never match"
                );
                assert!(!name.is_empty(), "the {label} list has an empty entry");
            }
            let mut seen = std::collections::BTreeSet::new();
            for name in *list {
                assert!(
                    seen.insert(*name),
                    "`{name}` appears twice in the {label} list"
                );
            }
        }
    }
}
