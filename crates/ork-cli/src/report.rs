//! `outlaw report` -- turn what went wrong into a bug report.
//!
//! The whole command is: show the person exactly what would be posted, and
//! then give them a link that opens the form with it already filled in. They
//! read it, edit it, and press the button.
//!
//! It does not post anything, and it does not want an account. That is the
//! design, not a stage on the way to something better: a reporter that can
//! publish for you is one that can publish your logs before you have read
//! them, and no amount of automatic redaction makes that a reasonable thing
//! for a diagnostic tool to be able to do.

use anyhow::{Context as _, Result};
use ork_core::incident::{self, report::Context};

use crate::render::wrap;
use crate::style::{bold, dim};

/// Everything the report should say about this machine.
fn context() -> Context {
    let host = ork_core::platform::detect()
        .and_then(|platform| Ok((platform.kind().to_string(), platform.host()?)))
        .ok();

    Context {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: host
            .as_ref()
            .map(|(kind, _)| kind.clone())
            .unwrap_or_default(),
        os_name: host
            .as_ref()
            .map(|(_, host)| host.os_name.clone())
            .unwrap_or_default(),
        architecture: host
            .as_ref()
            .map(|(_, host)| host.arch.clone())
            .unwrap_or_default(),
        extra: Vec::new(),
    }
}

pub fn run(open: bool, save: Option<std::path::PathBuf>, clear: bool, json: bool) -> Result<()> {
    let state_dir = crate::fix::state_dir()?;

    if clear {
        incident::clear(&state_dir).context("could not clear the recorded problems")?;
        if !json {
            println!("Cleared. Nothing recorded is kept.");
        }
        return Ok(());
    }

    let report = incident::report::build(&state_dir, &context());

    if let Some(path) = &save {
        std::fs::write(path, &report.body)
            .with_context(|| format!("could not write {}", path.display()))?;
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // The full text, before anything else. Somebody about to post this is
    // entitled to read it first, and burying it under a link would encourage
    // exactly the habit this feature must not build.
    println!("{}", bold("This is what would be posted:"));
    println!();
    println!("{}", dim(&"-".repeat(66)));
    println!("{}", report.title);
    println!("{}", dim(&"-".repeat(66)));
    println!("{}", report.body.trim_end());
    println!("{}", dim(&"-".repeat(66)));
    println!();

    if report.is_empty() {
        println!("{}", dim("Nothing has gone wrong on this machine yet."));
        println!(
            "{}",
            dim("You can still use the link below to describe something by hand.")
        );
        println!();
    }

    if let Some(path) = &save {
        println!("Saved to {}", path.display());
        println!();
    }

    match &report.issue_url {
        Some(url) => {
            println!("{}", bold("Open this to post it:"));
            println!("{url}");
            println!();
            println!(
                "{}",
                dim("The form opens with all of the above already filled in. Nothing is sent")
            );
            println!(
                "{}",
                dim("until you press the button on that page yourself.")
            );

            if open {
                println!();
                match ork_core::platform::open_url(url) {
                    Ok(()) => println!("{}", dim("Opening a browser...")),
                    // The link is right there above, so this is a nuisance
                    // rather than a failure.
                    Err(error) => {
                        println!("{}", dim(&format!("Could not open a browser: {error}")))
                    }
                }
            }
        }
        // Too long for a link. Rather than quietly posting half of it, the
        // report is written out and attached by hand.
        None => {
            let path = save.unwrap_or_else(|| state_dir.join("problem-report.md"));
            if let Err(error) = std::fs::write(&path, &report.body) {
                println!("{}", dim(&format!("Could not save the report: {error}")));
            } else {
                println!("Saved to {}", path.display());
            }
            println!();
            for line in wrap(
                "This report is too long to carry in a link without cutting the end off it, \
                 which would lose the part that usually matters. Open the form below and \
                 attach the saved file, or paste it in.",
                72,
            ) {
                println!("{}", dim(&line));
            }
            println!();
            println!("{}", report.issue_form_url);
        }
    }

    Ok(())
}

/// A one-line nudge, printed after something has gone wrong.
///
/// Only when there is something to report. A tool that suggests filing a bug
/// after every run trains people to ignore it.
pub fn hint_if_anything_recorded() {
    let Ok(state_dir) = crate::fix::state_dir() else {
        return;
    };
    let recorded = incident::recent(&state_dir, 1);
    if recorded.is_empty() {
        return;
    }
    eprintln!();
    eprintln!(
        "{}",
        dim("That was recorded. `outlaw report` turns it into a bug report you can post.")
    );
}
