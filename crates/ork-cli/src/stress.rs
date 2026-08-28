//! `outlaw stress` -- work the machine hard on purpose, and watch what it does.
//!
//! The behaviour is all in [`ork_core::stress`]; this is presentation, plus
//! the one thing a front-end owes this particular command: saying what is
//! about to happen to the machine *before* it happens, in numbers rather than
//! in adjectives. How long, how many cores, how much memory, how to stop it,
//! and whether anything is watching the temperature.
//!
//! Unlike a scan, this prints while it runs. Silence is right for a watcher --
//! it means nothing has changed -- and wrong here, because somebody sitting in
//! front of a machine whose fans have just come up needs to see that the thing
//! doing it is still under control and can be stopped.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ork_core::stress::{
    Ending, MemorySummary, Plan, StressEvent, StressReport, StressTest, memory,
};
use ork_core::util::format_bytes;
use tokio::sync::mpsc;

use crate::render;
use crate::style::{bold, dim};

/// Run the test.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    cpu: bool,
    memory_test: bool,
    minutes: u64,
    memory_share: f64,
    threads: Option<usize>,
    json: bool,
    assume_yes: bool,
) -> Result<()> {
    let plan = Plan {
        cpu,
        memory: memory_test,
        duration: Duration::from_secs(minutes.saturating_mul(60)),
        memory_share,
        threads,
    };

    if plan.is_empty() {
        // Rather than running for ten minutes doing nothing and reporting that
        // nothing went wrong, which would be true and useless.
        anyhow::bail!(
            "nothing to test: --no-cpu and --no-memory together leave this with no work to do"
        );
    }

    let platform = ork_core::platform::detect()?;

    if !json {
        preamble(&plan, platform.as_ref())?;
        if !assume_yes && !confirmed()? {
            println!("{}", dim("Nothing was run."));
            return Ok(());
        }
    }

    let (sender, mut events) = mpsc::unbounded_channel();
    let mut test = StressTest::new(plan).with_events(sender);
    let cancel = test.cancel_token();

    // Ctrl-C stops the test rather than killing the process, so the report is
    // still written and the machine is not left with work detached from
    // anything watching it.
    let interrupt = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            interrupt.cancel();
        }
    });

    let printer = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if json {
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
                continue;
            }
            show(&event);
        }
    });

    let report = test.run(Arc::clone(&platform)).await?;
    // Dropped before waiting on the printer: the test holds the sending end of
    // the channel the printer is reading, and while it is alive that channel
    // never closes and the printer never finishes.
    drop(test);
    let _ = printer.await;

    if !json {
        println!();
        summary(&report);
    }
    Ok(())
}

/// Say what is about to happen, in numbers.
fn preamble(plan: &Plan, platform: &dyn ork_core::Platform) -> Result<()> {
    println!("{}", bold("About to work this machine hard."));
    println!(
        "  For {} minute{}, or until you press Ctrl-C, whichever is first.",
        plan.duration.as_secs() / 60,
        if plan.duration.as_secs() / 60 == 1 {
            ""
        } else {
            "s"
        }
    );

    if plan.cpu {
        let cores = plan.threads.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1)
        });
        println!(
            "  {cores} core{} at full load.",
            if cores == 1 { "" } else { "s" }
        );
    }

    if plan.memory {
        let available = platform.memory()?.available_bytes;
        match memory::budget(available, plan.memory_share) {
            memory::Budget::Test { bytes } => println!(
                "  {} of memory filled and checked, of the {} free. {} is always left alone.",
                format_bytes(bytes),
                format_bytes(available),
                format_bytes(memory::RESERVED_BYTES),
            ),
            memory::Budget::NotEnoughSpare { .. } => println!(
                "  The memory will not be tested: only {} is free, and this always leaves {} \
                 for the machine to keep running in.",
                format_bytes(available),
                format_bytes(memory::RESERVED_BYTES),
            ),
        }
    }

    println!(
        "{}",
        dim(
            "  The machine will get hot and will be slow to use while this runs. Nothing is \
             changed and nothing is written; it stops itself if anything reaches the \
             temperature this machine says is critical."
        )
    );
    Ok(())
}

/// Ask before heating somebody's computer.
///
/// The command was typed deliberately, so this is close to a formality -- but
/// it is the only place the numbers above can be read before rather than after
/// the fans come up, and `--yes` is there for anything scripted.
fn confirmed() -> Result<bool> {
    use std::io::Write;
    print!("Start? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return Ok(false);
    }
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn show(event: &StressEvent) {
    match event {
        StressEvent::Started {
            seconds,
            cpu_threads,
            memory_bytes,
            watching_heat,
        } => {
            let mut parts = Vec::new();
            if *cpu_threads > 0 {
                parts.push(format!("{cpu_threads} cores"));
            }
            if *memory_bytes > 0 {
                parts.push(format_bytes(*memory_bytes));
            }
            let minutes = seconds / 60;
            println!(
                "\n{} {} for {minutes} minute{}. Ctrl-C to stop.",
                bold("Running."),
                parts.join(" and "),
                if minutes == 1 { "" } else { "s" }
            );
            if !*watching_heat {
                // Before the run, not buried in the report afterwards.
                // Somebody about to heat a laptop should know now that nothing
                // is watching how hot it gets.
                println!(
                    "{}",
                    dim(&format!(
                        "  This machine reports no temperature that can be believed, so \
                         nothing is watching for overheating and the run cannot stop \
                         itself.{}",
                        if cfg!(windows) {
                            " On Windows that reading needs administrator rights; running \
                             elevated may give one."
                        } else {
                            ""
                        }
                    ))
                );
            }
        }
        StressEvent::Progress {
            elapsed_secs,
            total_secs,
            blocks,
            memory_patterns,
            hottest,
        } => {
            let heat = match hottest {
                Some(heat) => format!("  {} {:.0}C", heat.label, heat.peak_c),
                None => String::new(),
            };
            // One line, rewritten, so a ten-minute run does not leave three
            // hundred lines of progress above the result.
            print!(
                "\r  {elapsed_secs}s of {total_secs}s   {blocks} blocks   {memory_patterns} memory patterns{heat}    "
            );
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        StressEvent::Fault { fault } => {
            // On its own line, immediately, and never overwritten by the
            // progress line that would otherwise land on top of it.
            println!("\n{} {}", bold("Fault."), fault.detail);
        }
        StressEvent::Finished { .. } => {}
    }
}

/// What happened, once it is over.
fn summary(report: &StressReport) {
    match &report.ending {
        Ending::Completed => println!("{} after {}.", bold("Finished"), report.ran_for()),
        Ending::Cancelled => println!("{} after {}.", bold("Stopped"), report.ran_for()),
        Ending::TooHot {
            sensor,
            reached_c,
            ceiling_c,
        } => println!(
            "{} {sensor} reached {reached_c:.0}C, past the {ceiling_c:.0}C this machine says \
             is its limit. Ran for {}.",
            bold("Stopped because it got too hot."),
            report.ran_for()
        ),
    }

    if let Some(cpu) = &report.cpu {
        println!(
            "  {} cores, {} blocks, {} wrong.",
            cpu.threads, cpu.blocks, cpu.wrong
        );
    }
    match &report.memory {
        Some(MemorySummary::Ran {
            bytes,
            patterns,
            mismatches,
        }) => println!(
            "  {} of memory, {patterns} pattern{} checked, {} bad.",
            format_bytes(*bytes),
            if *patterns == 1 { "" } else { "s" },
            mismatches.len()
        ),
        Some(MemorySummary::NotRun { .. }) => {}
        None => {}
    }

    if report.watched_heat {
        let peaks: Vec<String> = report
            .heat
            .iter()
            .take(4)
            .map(|heat| format!("{} {:.0}C", heat.label, heat.peak_c))
            .collect();
        println!("  {} {}", dim("Hottest:"), peaks.join(", "));
    }

    println!();
    // Through the same renderer the scan uses, so a fault found here reads
    // exactly like a fault found anywhere else -- and so there is one place
    // that decides how a finding is written.
    let findings = report.findings();
    if findings.is_empty() {
        println!("{}", dim("Nothing to report."));
    } else {
        for finding in &findings {
            render::finding(finding);
        }
    }
}
