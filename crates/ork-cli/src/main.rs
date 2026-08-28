//! Command-line front-end for the Outlaw Repair Kit.
//!
//! This binary is deliberately thin. Everything it can do is a call into
//! `ork-core`, so the desktop app and the daemon expose the same capabilities
//! without reimplementing anything.

mod ai;
mod boot;
mod fix;
mod link;
mod refusal;
mod render;
mod report;
mod stress;
mod style;
mod watch;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ork_core::{ScanTier, Scanner};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(
    name = "outlaw",
    version,
    about = "Scan a computer for hardware and software problems, in plain language.",
    long_about = None
)]
struct Cli {
    /// Print machine-readable JSON instead of a human-readable report.
    #[arg(long, global = true)]
    json: bool,

    /// Log level for internal diagnostics: error, warn, info, debug, trace.
    #[arg(long, global = true, default_value = "warn")]
    log: String,

    /// Skip the start-up screen, self-test, and update check.
    #[arg(long, global = true)]
    no_boot: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a scan and report what is wrong.
    Scan {
        /// How thorough to be: quick, full, or deep.
        ///
        /// No tier has a time limit; press Ctrl-C to stop. `full` adds disk
        /// health, launch tests, and what starts with the machine. `deep`
        /// adds the system file check, which reads and hashes most of the
        /// operating system and takes minutes to an hour. No tier runs the
        /// stress and burn-in test -- that is `outlaw stress`, asked for on
        /// its own.
        #[arg(long, short, default_value = "quick")]
        tier: ScanTier,

        /// Also explain the findings, using runbooks and, if one is
        /// configured, a model.
        #[arg(long)]
        explain: bool,
    },
    /// Keep looking, and say something only when something changes.
    ///
    /// A quiet watcher is a working watcher: the first look records how the
    /// machine is now, and after that nothing is printed unless a problem
    /// appears, gets worse, or goes away. There is no time limit; press Ctrl-C
    /// to stop.
    Watch {
        /// How thorough each look should be. Quick by default, deliberately:
        /// a check heavy enough to be felt should be asked for, not arrive
        /// behind your work every quarter of an hour.
        #[arg(long, short, default_value = "quick")]
        tier: ScanTier,

        /// How many minutes between looks. Raised to one minute if lower.
        #[arg(long, default_value = "15", value_name = "MINUTES")]
        every: u64,

        /// Take one look and stop, for running from a scheduled task.
        #[arg(long)]
        once: bool,
    },
    /// Show what the watcher remembers, without watching.
    Watching,
    /// Work the machine hard on purpose, and see whether it gets anything wrong.
    ///
    /// This is the one command here that acts on the hardware rather than
    /// observing it: every core is loaded and most of the free memory is
    /// filled, deliberately, for as long as you ask. It exists for the faults
    /// that observation cannot see -- memory that corrupts a bit an hour, a
    /// core that computes wrongly only when hot, a cooling system full of dust
    /// -- which are the faults that get mistaken for bad software.
    ///
    /// The machine gets hot and is slow to use while it runs. Nothing is
    /// changed and nothing is written. It stops itself if any part of the
    /// machine reaches the temperature that machine says is critical, and
    /// Ctrl-C stops it at any moment.
    Stress {
        /// How many minutes to run for. Not a limit on work that would
        /// otherwise continue -- it is the work.
        #[arg(long, default_value = "10", value_name = "MINUTES")]
        minutes: u64,

        /// Leave the processor alone and only test the memory.
        #[arg(long)]
        no_cpu: bool,

        /// Leave the memory alone and only work the processor.
        #[arg(long)]
        no_memory: bool,

        /// What share of free memory to test, from 0.05 to 0.95. A gigabyte is
        /// always left for the machine to keep running in, whatever this says.
        #[arg(long, default_value = "0.6", value_name = "SHARE")]
        memory_share: f64,

        /// How many cores to work. All of them by default.
        #[arg(long, value_name = "COUNT")]
        threads: Option<usize>,

        /// Start without asking. For scripts and scheduled tasks.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Show which model would be used, and why.
    Models,
    /// Show problems waiting to be worked through.
    Queue,
    /// Work through the triage queue.
    ///
    /// A dry run unless --apply is given. Even then, every change is
    /// confirmed individually before it happens.
    Fix {
        /// Allow changes to be made, after confirming each one.
        #[arg(long)]
        apply: bool,
    },
    /// Show everything the tool has checked, found, attempted, and changed.
    Audit {
        /// How many entries to show.
        #[arg(long, default_value = "40")]
        limit: usize,
    },
    /// Turn a crash or an error into a bug report you can post.
    ///
    /// Shows exactly what would be posted, with personal details removed, and
    /// gives you a link that opens the form already filled in. Nothing is ever
    /// sent for you.
    Report {
        /// Open the prefilled issue form in a browser.
        #[arg(long)]
        open: bool,

        /// Also write the report to a file.
        #[arg(long, value_name = "PATH")]
        save: Option<std::path::PathBuf>,

        /// Forget everything recorded so far, and report nothing.
        #[arg(long)]
        clear: bool,
    },
    /// Show where settings live and what they currently say.
    Config,
    /// Store a credential in the system credential store.
    ///
    /// The value is read from standard input, never from an argument, so it
    /// does not end up in shell history.
    SetKey {
        /// Which credential: `cloud` or `remote`.
        which: String,

        /// Remove the stored credential instead of setting one.
        #[arg(long)]
        remove: bool,
    },
    /// Lend a model to another machine, or borrow one.
    ///
    /// A linked machine can be asked to think about a problem and to say what
    /// its last scan found. It cannot be made to do anything: no command in
    /// the link changes the machine at the other end.
    Link {
        #[command(subcommand)]
        action: Option<LinkAction>,
    },
    /// Run the start-up screen on its own: self-test and update check.
    Boot,
    /// List the checks this build knows how to run.
    Probes,
    /// Show what this tool detected about the machine it is running on.
    Host,
    /// Read the manual, which is carried inside this program.
    ///
    /// No page name lists what there is. The pages are the same ones the
    /// window shows and the same ones in `docs/` -- compiled in, so they are
    /// readable on a machine that cannot reach the internet, which is a
    /// machine this tool expects to be run on.
    Docs {
        /// Which page, e.g. `commands`. Omit to list them.
        page: Option<String>,
    },
}

#[derive(Subcommand)]
enum LinkAction {
    /// Lend this machine's model to machines that pair with it.
    Host {
        #[arg(long, default_value_t = ork_link::DEFAULT_PORT)]
        port: u16,

        /// The model to lend. Defaults to the first local address in settings.
        #[arg(long)]
        model_url: Option<String>,

        /// Do not answer discovery on the local network.
        #[arg(long)]
        no_discovery: bool,
    },
    /// Pair with a machine that is showing a pairing code.
    Join {
        /// The code from the other machine's screen. Asked for if left out.
        code: Option<String>,

        /// Where that machine is. Found on the local network if left out.
        #[arg(long)]
        at: Option<String>,

        #[arg(long, default_value_t = ork_link::DEFAULT_PORT)]
        port: u16,
    },
    /// See who on this network is lending a model.
    Find {
        #[arg(long, default_value_t = ork_link::DEFAULT_PORT)]
        port: u16,
    },
    /// Ask every linked machine whether it is still answering.
    Check,
    /// See what is wrong with a linked machine, without touching it.
    View {
        /// Which machine. The first linked one if left out.
        name: Option<String>,
    },
    /// Cut a link and forget its token.
    Remove {
        /// The machine's name, or its id.
        name: String,
    },
}

/// Set up logging, and start keeping a record of anything that goes wrong.
///
/// The console filter is attached to the console layer alone, so how chatty
/// somebody asked the terminal to be has no bearing on what gets recorded. An
/// error worth reporting is worth recording whether or not it was printed.
fn start_recording(level: &str) {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_env("ORK_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    let state_dir = fix::state_dir().ok();
    if let Some(dir) = state_dir.clone() {
        ork_core::incident::catch_crashes(dir);
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(filter),
        )
        .with(state_dir.map(ork_core::incident::IncidentLayer::new))
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    start_recording(&cli.log);

    let outcome = dispatch(cli).await;

    // A command that *broke* is exactly the thing worth reporting, and until
    // this point nothing had recorded it: an error returned up the stack is
    // printed by the caller, never logged, so the layer above never saw it.
    // Recorded here as an error like any other, so there is one path in.
    //
    // A refusal is not that. "Say which machine with --at" is the tool working
    // correctly, and recording it would fill the list of things worth
    // reporting with things that are not -- until somebody posts one as an
    // issue, having been told by the program that it was worth reporting.
    if let Err(error) = &outcome
        && !refusal::Refusal::is_one(error)
    {
        tracing::error!(target: "outlaw::command", "{error:#}");
        report::hint_if_anything_recorded();
    }
    outcome
}

async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Link { action } => match action {
            None => link::show(cli.json, false).await,
            Some(LinkAction::Host {
                port,
                model_url,
                no_discovery,
            }) => link::host(port, model_url, !no_discovery).await,
            Some(LinkAction::Join { code, at, port }) => link::join(code, at, port).await,
            Some(LinkAction::Find { port }) => link::find(port, cli.json).await,
            Some(LinkAction::Check) => link::show(cli.json, true).await,
            Some(LinkAction::View { name }) => link::view(name, cli.json).await,
            Some(LinkAction::Remove { name }) => link::remove(&name),
        },
        Command::Boot => {
            // Quietly when the answer is being read by something rather than
            // somebody: the banner and the progress pane are drawn on stdout,
            // and JSON with a picture in front of it is not JSON. Asked for
            // explicitly, so `--no-boot` has no say here.
            let report = if cli.json {
                boot::run_quietly().await
            } else {
                boot::run().await
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            if report.ready() {
                Ok(())
            } else {
                std::process::exit(1)
            }
        }
        Command::Scan { tier, explain } => {
            // The start-up screen belongs on the commands a person sits and
            // watches. Putting a network update check in front of `config`
            // would make a one-line answer take four seconds.
            start_up(&cli, false).await;
            run_scan(tier, cli.json, explain).await
        }
        Command::Watch { tier, every, once } => {
            // A watcher is something somebody sits and leaves running, so the
            // start-up screen belongs here for the same reason it belongs on a
            // scan. Not for `--once`, though: that is run from a scheduled
            // task, where there is nobody to read a splash screen and the only
            // thing that should reach a log is what changed.
            if !once {
                start_up(&cli, false).await;
            }
            watch::run(tier, every, cli.json, once).await
        }
        Command::Watching => watch::status(cli.json),
        Command::Stress {
            minutes,
            no_cpu,
            no_memory,
            memory_share,
            threads,
            yes,
        } => {
            // Something somebody sits and watches, like a scan, so the
            // start-up screen belongs here too.
            start_up(&cli, false).await;
            stress::run(
                !no_cpu,
                !no_memory,
                minutes,
                memory_share,
                threads,
                cli.json,
                yes,
            )
            .await
        }
        Command::Docs { page } => render::docs(page, cli.json),
        Command::Probes => render::probes(cli.json),
        Command::Host => render::host(cli.json),
        Command::Models => ai::show_models(cli.json).await,
        Command::Queue => fix::show_queue(cli.json),
        Command::Fix { apply } => {
            start_up(&cli, apply).await;
            fix::work_queue(apply, cli.json).await
        }
        Command::Audit { limit } => fix::show_audit(limit, cli.json),
        Command::Report { open, save, clear } => report::run(open, save, clear, cli.json),
        Command::Config => ai::show_config(cli.json),
        Command::SetKey { which, remove } => {
            if remove {
                ai::clear_key(&which)
            } else {
                ai::set_key(&which)
            }
        }
    }
}

/// Show the boot screen, unless this run has no business showing one.
///
/// `required` is set for runs that change the machine: if the tool cannot
/// vouch for its own state database or snapshot area, it must not start
/// applying fixes, because the promise to roll back would be empty.
/// Whether to run the start-up self-test, and whether to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boot {
    /// Do not run it. Only `--no-boot` asks for this.
    Skip,
    /// Run it without printing anything.
    Quietly,
    /// Run it with the boot screen.
    OnScreen,
}

/// Decide from the flags alone, so the decision can be tested.
///
/// It was wrong, and invisibly so. `--json` skipped the self-test outright,
/// which quietly took the guard in [`start_up`] with it: a run that changes
/// the machine got no self-test at all, so the one case that guard exists for
/// -- a snapshot area the tool cannot vouch for -- sailed straight through as
/// long as somebody asked for JSON. Asking for machine-readable output is a
/// statement about the output, not about which safety checks apply.
fn boot_style(no_boot: bool, json: bool) -> Boot {
    if no_boot {
        Boot::Skip
    } else if json {
        Boot::Quietly
    } else {
        Boot::OnScreen
    }
}

async fn start_up(cli: &Cli, required: bool) {
    let report = match boot_style(cli.no_boot, cli.json) {
        Boot::Skip => return,
        Boot::Quietly => boot::run_quietly().await,
        Boot::OnScreen => boot::run().await,
    };
    if required && !report.ready() {
        eprintln!("Refusing to change anything while the self-test is failing.");
        std::process::exit(1);
    }
}

async fn run_scan(tier: ScanTier, json: bool, explain: bool) -> Result<()> {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let scanner = Scanner::new()?.with_events(events_tx);

    // Ctrl-C is the manual cancel. Nothing else stops a scan early: a check
    // that is still doing real work is never cut off for taking too long.
    let cancel = scanner.cancel_token();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\nStopping after the current check finishes...");
            cancel.cancel();
        }
    });

    // Progress goes to stderr so that `--json` on stdout stays pipeable.
    let quiet = json;
    let progress = tokio::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            if !quiet {
                render::progress(&event);
            }
        }
    });

    let report = scanner.run(tier).await?;
    drop(scanner);
    let _ = progress.await;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render::report(&report);
    }

    // Complex problems go on the triage queue rather than blocking the scan.
    match fix::enqueue_from_scan(&report) {
        Ok(added) if added > 0 && !json => {
            println!(
                "{} added to the triage queue. Run `outlaw fix` to work through them.",
                added
            );
            println!();
        }
        Ok(_) => {}
        // Failing to queue must not lose the user their scan results.
        Err(error) => tracing::warn!(%error, "could not update the triage queue"),
    }

    if explain {
        ai::explain(&report, json).await?;
    }

    // A non-zero exit code lets this be used in a script or a scheduled task
    // without parsing the output.
    if report
        .worst_severity()
        .is_some_and(|worst| worst >= ork_core::Severity::High)
    {
        std::process::exit(2);
    }
    Ok(())
}

/// Tests that the documentation still describes the program.
///
/// Documentation goes stale silently. A command gains an option, a page gains
/// a section, a release goes out, and the only thing that notices is somebody
/// following instructions that no longer work -- by which point they have
/// concluded the tool is broken, which is a reasonable thing to conclude.
///
/// These check the parts that can be checked mechanically, so that keeping the
/// manual honest is a build failure rather than a thing to remember.
#[cfg(test)]
mod startup {
    use super::{Boot, boot_style};

    #[test]
    fn machine_readable_output_still_runs_the_self_test() {
        // The bug this replaced: `--json` meant no self-test at all, so
        // `outlaw --json fix --apply` would change the machine without the
        // check that refuses to when the snapshot area cannot be vouched for.
        assert_eq!(boot_style(false, true), Boot::Quietly);
    }

    #[test]
    fn only_no_boot_skips_the_self_test() {
        // One flag turns it off, and it is the one named after doing so.
        assert_eq!(boot_style(true, false), Boot::Skip);
        assert_eq!(boot_style(true, true), Boot::Skip);
    }

    #[test]
    fn a_person_at_a_terminal_gets_the_boot_screen() {
        assert_eq!(boot_style(false, false), Boot::OnScreen);
    }
}

/// The install scripts and the release workflow have to agree on file names.
///
/// Nothing else checks this, and the failure is silent and total: the workflow
/// publishes `outlaw-v0.9.0-x86_64-pc-windows-msvc.zip`, an installer that
/// asks for anything else gets a 404, and every person who ran the one-line
/// install command that day is told the download failed. It cannot be caught
/// by testing the tool, because the tool is fine -- it is the way people get
/// it that is broken, and the only person who finds out is a stranger.
///
/// Checked as text on both sides. Neither file can be executed here: one is a
/// PowerShell script that downloads from the internet and edits a PATH, and
/// the other is a shell script for an operating system this is not.
#[cfg(test)]
mod packaging {
    const PS1: &str = include_str!("../../../install/install.ps1");
    const SH: &str = include_str!("../../../install/install.sh");
    const RELEASE: &str = include_str!("../../../.github/workflows/release.yml");

    #[test]
    fn the_installers_ask_for_the_names_the_release_publishes() {
        // Each pair is (what an installer builds, what the workflow names).
        // Written with the shell variables still in them, because that is the
        // form both sides are in and normalising either one would be a third
        // opinion about what the name is.
        let pairs = [
            (
                "outlaw-$Version-$target.zip",
                r#"staging="outlaw-${version}-${{ matrix.target }}""#,
            ),
            (
                r#"outlaw-repair-kit-$Version-x64-setup.exe"#,
                r#"name="outlaw-repair-kit-${version}-x64-setup.exe""#,
            ),
        ];
        for (asked, published) in pairs {
            assert!(
                PS1.contains(asked),
                "install.ps1 no longer builds `{asked}`"
            );
            assert!(
                RELEASE.contains(published),
                "the release workflow no longer publishes `{published}`"
            );
        }

        assert!(
            SH.contains(r#"ASSET="outlaw-${VERSION}-${TARGET}.tar.gz""#),
            "install.sh no longer builds the archive name"
        );
        assert!(
            RELEASE.contains(r#"tar czf "${staging}.tar.gz""#),
            "the release workflow no longer publishes a .tar.gz"
        );
        assert!(
            SH.contains(r#"APPIMAGE="outlaw-repair-kit-${VERSION}-amd64.AppImage""#),
            "install.sh no longer builds the AppImage name"
        );
        assert!(
            RELEASE.contains(r#"name="outlaw-repair-kit-${version}-amd64.AppImage""#),
            "the release workflow no longer publishes an AppImage"
        );
    }

    #[test]
    fn both_installers_check_what_they_downloaded() {
        // The one promise the front page makes about installing: it refuses a
        // file whose checksum does not match the published one. A download
        // that stopped being checked would still install perfectly, which is
        // exactly why this is worth asserting rather than assuming.
        assert!(
            PS1.contains("SHA256SUMS"),
            "install.ps1 stopped fetching the digests"
        );
        assert!(
            PS1.contains("does not match its published checksum"),
            "install.ps1 stopped refusing a mismatched download"
        );
        assert!(
            SH.contains("SHA256SUMS"),
            "install.sh stopped fetching the digests"
        );
        assert!(
            SH.contains("does not match"),
            "install.sh stopped refusing a mismatched download"
        );
        assert!(
            RELEASE.contains("SHA256SUMS"),
            "the release workflow stopped publishing the digests both installers rely on"
        );
    }

    #[test]
    fn neither_installer_asks_for_administrator_rights() {
        // Also a promise on the front page. On Windows that means the user's
        // own PATH rather than the machine's; on Linux it means not writing
        // outside the home directory without being asked.
        assert!(
            PS1.contains(r#"SetEnvironmentVariable("Path", $updated, "User")"#),
            "install.ps1 no longer edits only the user's own PATH"
        );
        assert!(
            !PS1.contains(r#""Machine")"#),
            "install.ps1 is writing machine-wide environment variables"
        );
        assert!(
            !SH.contains("sudo install"),
            "install.sh is installing as root"
        );
    }
}

#[cfg(test)]
mod documentation {
    use super::Cli;
    use clap::CommandFactory;

    const COMMANDS_PAGE: &str = include_str!("../../../docs/commands.md");
    const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
    const README: &str = include_str!("../../../README.md");

    #[test]
    fn every_command_is_in_the_command_reference() {
        // The page is called "Command reference". A command missing from it is
        // a command nobody will find.
        let missing: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .filter(|name| name != "help")
            .filter(|name| !COMMANDS_PAGE.contains(&format!("`outlaw {name}")))
            .collect();

        assert!(
            missing.is_empty(),
            "these commands exist but are not in docs/commands.md: {missing:?}"
        );
    }

    #[test]
    fn the_command_reference_documents_nothing_that_does_not_exist() {
        // The other direction, and the worse one: instructions for a command
        // that was removed send somebody chasing an error message.
        let known: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();

        let documented: Vec<String> = COMMANDS_PAGE
            .lines()
            .filter_map(|line| line.strip_prefix("## `outlaw "))
            .filter_map(|rest| rest.split([' ', '`', '<']).next())
            .map(|name| name.to_string())
            .filter(|name| !name.is_empty())
            .collect();

        let phantom: Vec<&String> = documented
            .iter()
            .filter(|name| !known.contains(name))
            .collect();
        assert!(
            phantom.is_empty(),
            "docs/commands.md documents commands that do not exist: {phantom:?}"
        );
        assert!(
            !documented.is_empty(),
            "the parser found no documented commands, which means it is broken"
        );
    }

    #[test]
    fn the_command_reference_counts_the_self_checks_right() {
        // It said six when there were seven. A number in prose is the easiest
        // thing in a manual to leave behind, and the hardest for a reader to
        // doubt.
        let claimed = COMMANDS_PAGE
            .split_whitespace()
            .zip(COMMANDS_PAGE.split_whitespace().skip(1))
            .find(|(_, next)| next.starts_with("self-checks"))
            .and_then(|(count, _)| count.parse::<usize>().ok())
            .expect("the page should say how many self-checks there are");
        assert_eq!(
            claimed,
            ork_boot::selftest::CHECK_COUNT,
            "docs/commands.md claims {claimed} self-checks and there are {}",
            ork_boot::selftest::CHECK_COUNT
        );
    }

    #[test]
    fn the_changelog_has_an_entry_for_this_version() {
        // A release that changed nothing anybody can read about is a release
        // nobody can decide whether to install.
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            CHANGELOG.contains(&format!("## v{version}")),
            "CHANGELOG.md has no `## v{version}` section, but that is the version being built"
        );
    }

    #[test]
    fn the_newest_changelog_entry_is_this_version() {
        // Catches the other half: a version bumped without the changelog
        // moving, which leaves the newest entry describing the release before
        // this one.
        let newest = CHANGELOG
            .lines()
            .find_map(|line| line.strip_prefix("## v"))
            .expect("the changelog has at least one version heading");
        assert_eq!(
            newest.trim(),
            env!("CARGO_PKG_VERSION"),
            "the newest changelog entry is not the version being built"
        );
    }

    #[test]
    fn the_front_page_lists_every_check_this_build_runs() {
        // The front page has a table of what a scan looks at, and it is where
        // somebody decides whether this tool is worth downloading. It had
        // fallen a check behind: the start-up check was described further down
        // in the roadmap and missing from the table, so the one place people
        // actually read undersold what the tool does.
        //
        // Matched on the check's own name rather than on wording, so the table
        // can be written however reads best and still has to mention each one.
        let mut missing: Vec<String> = ork_core::probes::all_meta()
            .iter()
            .map(|meta| meta.name.to_string())
            .filter(|name| !README.contains(name.as_str()))
            .collect();
        missing.sort();
        missing.dedup();

        assert!(
            missing.is_empty(),
            "README.md does not mention these checks: {missing:?}"
        );
    }

    #[test]
    fn the_front_page_still_points_at_pages_that_exist() {
        // A broken link on the front page is the first thing a stranger meets.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let mut broken = Vec::new();
        for line in README.lines() {
            let mut rest = line;
            while let Some(at) = rest.find("](") {
                let after = &rest[at + 2..];
                let Some(end) = after.find(')') else { break };
                let target = &after[..end];
                rest = &after[end..];
                // Only local files. Anchors and URLs are somebody else's
                // problem, and checking them would need the network.
                if target.starts_with("http") || target.starts_with('#') || target.is_empty() {
                    continue;
                }
                let path = target.split('#').next().unwrap_or(target);
                if !root.join(path).exists() {
                    broken.push(path.to_string());
                }
            }
        }
        assert!(
            broken.is_empty(),
            "README.md links to missing files: {broken:?}"
        );
    }
}
