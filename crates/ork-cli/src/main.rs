//! Command-line front-end for the Outlaw Repair Kit.
//!
//! This binary is deliberately thin. Everything it can do is a call into
//! `ork-core`, so the desktop app and the daemon expose the same capabilities
//! without reimplementing anything.

mod ai;
mod boot;
mod fix;
mod link;
mod render;
mod report;
mod style;

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
        /// No tier has a time limit; press Ctrl-C to stop. `deep` currently
        /// runs exactly what `full` runs -- the stress and burn-in checks it
        /// is meant for are not built yet.
        #[arg(long, short, default_value = "quick")]
        tier: ScanTier,

        /// Also explain the findings, using runbooks and, if one is
        /// configured, a model.
        #[arg(long)]
        explain: bool,
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

    // A command that fails is exactly the thing worth reporting, and until
    // this point nothing had recorded it: an error returned up the stack is
    // printed by the caller, never logged, so the layer above never saw it.
    // Recorded here as an error like any other, so there is one path in.
    if let Err(error) = &outcome {
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
            let report = boot::run().await;
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
async fn start_up(cli: &Cli, required: bool) {
    if cli.no_boot || cli.json {
        return;
    }

    let report = boot::run().await;
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
