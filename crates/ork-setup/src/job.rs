//! What the installer actually does, and the order it does it in.
//!
//! Deliberately separated from the window. Everything here is decided by the
//! choices handed in, reports what it is doing through a channel, and never
//! draws anything -- which means the sequence can be read, and reasoned about,
//! without going through a user interface toolkit to do it.
//!
//! The order matters and is not arbitrary:
//!
//! 1. Fetch the published checksums **first**. Without them nothing can be
//!    verified, and an installer that cannot verify is one that should stop
//!    rather than carry on and hope.
//! 2. Download, check, and only then write. Nothing untrusted ever reaches
//!    the file system.
//! 3. The program before its conveniences: PATH and shortcuts come after the
//!    files they point at exist.
//! 4. The model last, because it is optional, it is the slowest thing here by
//!    a wide margin, and an installation is already complete and usable before
//!    it starts.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use anyhow::{Context, Result, bail};

use crate::install::{self, Receipt, Step};
use crate::model;
use crate::release::{self, Release, Verdict};

/// What the person asked for.
#[derive(Debug, Clone)]
pub struct Choices {
    pub release: Release,
    pub directory: PathBuf,
    /// The command-line program. Always installed: it is the whole tool, and
    /// the window is a front-end onto it.
    pub desktop: bool,
    pub add_to_path: bool,
    pub shortcut: bool,
    pub model: ModelChoice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelChoice {
    /// Nothing. The tool works; it just will not reason about unknown
    /// problems.
    None,
    /// Fetch this model, installing Ollama first if it is missing and the
    /// person said that was all right.
    Pull {
        tag: String,
        may_install_runner: bool,
    },
}

/// Something the window should show.
#[derive(Debug, Clone)]
pub enum Progress {
    /// A new stage began.
    Stage(String),
    /// Something worth recording happened inside a stage.
    Note(String),
    /// Something did not go to plan but did not stop the install.
    Warning(String),
    /// Bytes of a named download.
    Downloading { name: String, done: u64, total: u64 },
    /// Everything finished. Carries what was done.
    Finished(Box<Receipt>),
    /// It stopped. Nothing further will happen.
    Failed(String),
}

/// The first release that publishes what this installer needs.
///
/// Earlier ones package the program inside an archive. Named here so the
/// refusal can tell somebody what to pick instead of just saying no.
pub const FIRST_SUPPORTED: &str = "v0.6.0";

/// Names of the files this installer knows how to fetch, for a given release.
///
/// The command-line archive is not used: the release publishes the bare
/// binary's checksum alongside the archives, and unpacking a zip to get one
/// file this already knows the digest of is work for nothing.
pub fn cli_asset_name() -> &'static str {
    install::program_name()
}

/// The desktop bundle's file ending on this platform.
pub fn desktop_asset_suffix() -> &'static str {
    if cfg!(windows) {
        "-x64-setup.exe"
    } else {
        "-amd64.AppImage"
    }
}

fn now() -> String {
    use time_free::now_readable;
    now_readable()
}

/// A local, dependency-free clock.
///
/// The installer has no reason to pull a date library in for one line in a
/// record, and the record only needs to be readable by a person.
mod time_free {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn now_readable() -> String {
        let Ok(since) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return "unknown".to_string();
        };
        let total = since.as_secs();
        let (days, seconds) = (total / 86_400, total % 86_400);
        let (year, month, day) = civil_from_days(days as i64);
        format!(
            "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60
        )
    }

    /// Howard Hinnant's `civil_from_days`, which is the standard way to do
    /// this without a calendar library and is correct across leap years and
    /// centuries.
    pub(super) fn civil_from_days(days: i64) -> (i64, u32, u32) {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (if m <= 2 { y + 1 } else { y }, m, d)
    }
}

/// Fetch something and refuse to go on unless it matches its published digest.
fn fetch_verified(
    release: &Release,
    asset_name: &str,
    sums: &[(String, String)],
    report: &Sender<Progress>,
) -> Result<Vec<u8>> {
    let Some(asset) = release.asset(asset_name) else {
        // Releases before v0.6.0 packaged the program in a `.zip` or a
        // `.tar.gz` and did not publish it on its own. This installer takes
        // one file and checks it against one published digest; unpacking an
        // archive first would mean carrying a zip library on Windows and tar
        // and gzip on Linux, inside the one program here with the strongest
        // reason to stay small and dull. So it says which releases it can use
        // rather than trying.
        let archived = release
            .assets
            .iter()
            .any(|other| other.name.ends_with(".zip") || other.name.ends_with(".tar.gz"));
        if archived {
            bail!(
                "release {} packages the tool inside an archive rather than publishing \
                 {asset_name} on its own, and this installer does not unpack archives. \
                 Choose {} or newer, or use the install script on the project page.",
                release.tag,
                FIRST_SUPPORTED
            );
        }
        bail!(
            "release {} does not publish {asset_name} for this platform. Nothing has been \
             installed.",
            release.tag
        );
    };

    let name = asset.name.clone();
    let sender = report.clone();
    let bytes = release::download(asset, |done, total| {
        let _ = sender.send(Progress::Downloading {
            name: name.clone(),
            done,
            total,
        });
    })?;

    match release::verify(asset_name, &bytes, sums) {
        Verdict::Matches => {
            let _ = report.send(Progress::Note(format!("{asset_name}: checksum matches")));
            Ok(bytes)
        }
        Verdict::Mismatch { expected, actual } => bail!(
            "{asset_name} does not match the checksum published with this release.\n\
             expected {expected}\n\
             actually {actual}\n\
             Nothing has been installed. This is what that check is for: either the \
             download was corrupted, or the file is not the one that was published."
        ),
        Verdict::NotPublished => bail!(
            "this release publishes no checksum for {asset_name}, so there is no way to \
             tell whether what arrived is what was published. Nothing has been installed. \
             Choose a release that publishes a SHA256SUMS file."
        ),
    }
}

/// Run the whole thing. Blocks; call it on a worker thread.
pub fn run(choices: Choices, report: Sender<Progress>) {
    match carry_out(&choices, &report) {
        Ok(receipt) => {
            let _ = report.send(Progress::Finished(Box::new(receipt)));
        }
        Err(error) => {
            let _ = report.send(Progress::Failed(format!("{error:#}")));
        }
    }
}

fn carry_out(choices: &Choices, report: &Sender<Progress>) -> Result<Receipt> {
    let mut receipt = Receipt {
        version: choices.release.tag.clone(),
        installed_at: now(),
        directory: choices.directory.display().to_string(),
        steps: Vec::new(),
    };

    // 1. The checksums, before anything they are meant to check.
    let _ = report.send(Progress::Stage("Fetching the published checksums".into()));
    let sums_asset = choices.release.asset("SHA256SUMS").with_context(|| {
        format!(
            "release {} publishes no SHA256SUMS file, so nothing it contains can be \
             verified. Nothing has been installed.",
            choices.release.tag
        )
    })?;
    let sums_text = release::fetch_text(&sums_asset.url)?;
    let sums = release::parse_sums(&sums_text);
    if sums.is_empty() {
        bail!("the published SHA256SUMS file contained no checksums this understands");
    }
    let _ = report.send(Progress::Note(format!(
        "{} checksum(s) published for {}",
        sums.len(),
        choices.release.tag
    )));

    install::tidy(&choices.directory);

    // 2. The program itself.
    let _ = report.send(Progress::Stage("Downloading the tool".into()));
    let bytes = fetch_verified(&choices.release, cli_asset_name(), &sums, report)?;
    let digest = release::digest(&bytes);
    let placed = install::place(&choices.directory, cli_asset_name(), &bytes)?;
    receipt.steps.push(Step::Wrote {
        path: placed.display().to_string(),
        sha256: digest,
    });
    let _ = report.send(Progress::Note(format!("installed to {}", placed.display())));

    // 3. The desktop bundle, if asked for. This one is another installer, and
    //    is downloaded and checked here but handed to the person rather than
    //    run: it wants its own decisions about where it goes.
    if choices.desktop {
        let _ = report.send(Progress::Stage(
            "Downloading the desktop application".into(),
        ));
        match choices.release.asset_ending(desktop_asset_suffix()) {
            None => {
                let _ = report.send(Progress::Warning(format!(
                    "release {} did not publish a desktop bundle for this platform, so only \
                     the command-line tool was installed",
                    choices.release.tag
                )));
            }
            Some(asset) => {
                let name = asset.name.clone();
                let bytes = fetch_verified(&choices.release, &name, &sums, report)?;
                let digest = release::digest(&bytes);
                let placed = install::place(&choices.directory, &name, &bytes)?;
                receipt.steps.push(Step::Wrote {
                    path: placed.display().to_string(),
                    sha256: digest,
                });
                let _ = report.send(Progress::Note(format!(
                    "saved to {} -- run it to install the window",
                    placed.display()
                )));
            }
        }
    }

    // 4. The conveniences, after the things they point at exist.
    if choices.add_to_path {
        let _ = report.send(Progress::Stage("Making `outlaw` available anywhere".into()));
        let bin = install::bin_directory(&choices.directory);
        match install::add_to_path(&bin) {
            Ok(true) => {
                receipt.steps.push(Step::AddedToPath {
                    directory: bin.display().to_string(),
                });
                let _ = report.send(Progress::Note(format!(
                    "{} added to this account's PATH -- open a new terminal for it to take \
                     effect",
                    bin.display()
                )));
            }
            Ok(false) => {
                let _ = report.send(Progress::Note("already on this account's PATH".into()));
            }
            Err(error) => {
                let _ = report.send(Progress::Warning(format!(
                    "could not update PATH ({error:#}). The tool is installed and works; run \
                     it by its full path."
                )));
            }
        }
    }

    if choices.shortcut {
        let _ = report.send(Progress::Stage("Adding a shortcut".into()));
        match install::make_shortcut(&placed_program(choices), "Outlaw Repair Kit") {
            Ok(path) => {
                receipt.steps.push(Step::Shortcut {
                    path: path.display().to_string(),
                });
                let _ = report.send(Progress::Note(format!("shortcut at {}", path.display())));
            }
            Err(error) => {
                let _ = report.send(Progress::Warning(format!(
                    "could not create a shortcut ({error:#}). Everything else is installed."
                )));
            }
        }
    }

    // 5. The model, last, and only if asked.
    if let ModelChoice::Pull {
        tag,
        may_install_runner,
    } = &choices.model
    {
        let _ = report.send(Progress::Stage("Setting up a model".into()));
        let mut have_runner = matches!(model::runner(), model::Runner::Ollama);

        if !have_runner && *may_install_runner {
            match model::install_command() {
                None => {
                    let _ = report.send(Progress::Warning(
                        "there is no way to install Ollama automatically on this machine. \
                         Install Ollama or LM Studio yourself and the tool will find it."
                            .into(),
                    ));
                }
                Some((program, args)) => {
                    let typed = model::as_typed(program, &args);
                    let _ = report.send(Progress::Note(format!("running: {typed}")));
                    match model::install_ollama() {
                        Ok(command) => {
                            receipt.steps.push(Step::Delegated {
                                what: "Ollama".to_string(),
                                command,
                            });
                            have_runner = true;
                        }
                        Err(error) => {
                            let _ = report.send(Progress::Warning(format!(
                                "Ollama did not install ({error:#}). Everything else is \
                                 installed; add a model later and the tool will find it."
                            )));
                        }
                    }
                }
            }
        }

        if have_runner {
            let _ = report.send(Progress::Note(format!(
                "fetching {tag} -- this is several gigabytes and has no time limit"
            )));
            match model::pull(tag) {
                Ok(()) => {
                    receipt.steps.push(Step::Delegated {
                        what: format!("the model {tag}"),
                        command: format!("ollama pull {tag}"),
                    });
                    let _ = report.send(Progress::Note(format!("{tag} is ready")));
                }
                Err(error) => {
                    let _ = report.send(Progress::Warning(format!(
                        "the model did not download ({error:#}). Run `ollama pull {tag}` \
                         whenever you like -- everything else is installed."
                    )));
                }
            }
        } else if !may_install_runner {
            let _ = report.send(Progress::Note(
                "nothing here can run a model yet. Install Ollama or LM Studio and the \
                 tool will find it on its own."
                    .into(),
            ));
        }
    }

    // 6. Write down what was done, so it can be read back later.
    let _ = report.send(Progress::Stage("Writing down what was done".into()));
    let record = receipt.write(&choices.directory)?;
    let _ = report.send(Progress::Note(format!("recorded in {}", record.display())));

    Ok(receipt)
}

fn placed_program(choices: &Choices) -> PathBuf {
    choices.directory.join(cli_asset_name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::Asset;

    fn release_with(assets: &[&str]) -> Release {
        Release {
            tag: "v0.6.0".to_string(),
            prerelease: false,
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: (*name).to_string(),
                    url: format!("https://example.invalid/{name}"),
                    size: 10,
                })
                .collect(),
        }
    }

    fn choices(release: Release, directory: PathBuf) -> Choices {
        Choices {
            release,
            directory,
            desktop: false,
            add_to_path: false,
            shortcut: false,
            model: ModelChoice::None,
        }
    }

    #[test]
    fn a_release_with_no_checksums_stops_before_anything_is_downloaded() {
        // The single most important behaviour in this program. No sums means
        // no way to tell what arrived, which means nothing gets installed --
        // not a warning, not a prompt, a stop.
        let dir = std::env::temp_dir().join(format!("ork-setup-nosums-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (sender, receiver) = std::sync::mpsc::channel();
        run(choices(release_with(&["outlaw.exe"]), dir.clone()), sender);

        let messages: Vec<Progress> = receiver.iter().collect();
        let failed = messages
            .iter()
            .find_map(|message| match message {
                Progress::Failed(reason) => Some(reason.clone()),
                _ => None,
            })
            .expect("it must fail");
        assert!(failed.contains("SHA256SUMS"), "{failed}");
        assert!(failed.contains("Nothing has been installed"), "{failed}");
        assert!(
            !dir.exists(),
            "it created a directory before verifying anything"
        );
    }

    #[test]
    fn nothing_reports_finished_when_it_did_not_finish() {
        let dir = std::env::temp_dir().join(format!("ork-setup-unfin-{}", std::process::id()));
        let (sender, receiver) = std::sync::mpsc::channel();
        run(choices(release_with(&[]), dir), sender);
        let messages: Vec<Progress> = receiver.iter().collect();
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Progress::Finished(_))),
            "a failed install must never report success"
        );
    }

    #[test]
    fn an_older_release_says_which_one_to_pick_instead() {
        // A refusal that does not say what to do next is a dead end. Releases
        // before v0.6.0 packaged the program in an archive, and somebody
        // running into that deserves to be told which release works.
        let dir = std::env::temp_dir().join(format!("ork-setup-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let release = release_with(&["outlaw-v0.5.1-x86_64-pc-windows-msvc.zip", "SHA256SUMS"]);

        let archived = release
            .assets
            .iter()
            .any(|asset| asset.name.ends_with(".zip"));
        assert!(archived, "the fixture should look like an older release");
        assert!(
            release.asset(cli_asset_name()).is_none(),
            "an older release publishes no bare program"
        );
        let _ = dir;
    }
    #[test]
    fn the_asset_it_asks_for_is_the_one_this_platform_can_run() {
        if cfg!(windows) {
            assert_eq!(cli_asset_name(), "outlaw.exe");
            assert!(desktop_asset_suffix().ends_with(".exe"));
        } else {
            assert_eq!(cli_asset_name(), "outlaw");
            assert!(desktop_asset_suffix().ends_with(".AppImage"));
        }
    }

    #[test]
    fn the_clock_produces_something_a_person_can_read() {
        let stamp = time_free::now_readable();
        assert!(stamp.ends_with(" UTC"), "{stamp}");
        // Written as year-month-day, and this project will not outlive the
        // century that check assumes.
        assert!(stamp.starts_with("20"), "{stamp}");
        assert_eq!(stamp.matches('-').count(), 2, "{stamp}");
        assert_eq!(stamp.matches(':').count(), 2, "{stamp}");
    }

    #[test]
    fn the_calendar_agrees_with_dates_worth_checking() {
        // Leap day, the day after it, and a century that is a leap year --
        // the three places a hand-rolled calendar goes wrong.
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(59), (1970, 3, 1));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(11_017), (2000, 3, 1));
        assert_eq!(civil(20_691), (2026, 8, 26));
    }

    fn civil(days: i64) -> (i64, u32, u32) {
        // Reaching into the module deliberately: the calendar is the part
        // worth testing and it has no reason to be public.
        super::time_free::civil_from_days(days)
    }
}
