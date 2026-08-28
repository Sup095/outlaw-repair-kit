//! CLI surface for the model router and analysis layer.

use anyhow::{Context, Result};
use ork_ai::analysis::{AnalysisSource, Analyst};
use ork_ai::router::{ModelRouter, ModelTier, Routing, advise_for_vram};
use ork_ai::runbook::{Invasiveness, RunbookLibrary};
use ork_ai::secrets::{self, SecretKind};
use ork_core::Config;
use ork_core::scan::ScanReport;

use crate::render::wrap;
use crate::style::{bold, dim};

/// Where user-supplied runbooks live, beside the configuration file.
fn runbook_dir() -> Option<std::path::PathBuf> {
    Config::default_path()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("runbooks")))
}

/// Every file and folder this tool writes to, in one list.
///
/// Printed in full rather than summarised. The documentation tells people this
/// command shows where their data lives, and a list that quietly omitted two
/// of the places would make that a lie -- somebody checking what a diagnostic
/// tool keeps about them deserves the whole answer in one place. The list is
/// built here rather than gathered from each subsystem so that adding a new
/// store and forgetting to mention it is visible in one file.
fn where_things_live() -> Vec<(&'static str, std::path::PathBuf)> {
    let Ok(config) = Config::default_path() else {
        return Vec::new();
    };
    let Some(dir) = config.parent().map(std::path::Path::to_path_buf) else {
        return Vec::new();
    };
    vec![
        ("settings", config),
        ("queue and history", dir.join("state.db")),
        ("your runbooks", dir.join("runbooks")),
        ("backups before changes", dir.join("snapshots")),
        ("what the watcher knows", dir.join("watch-baseline.json")),
        ("paired machines", dir.join("peers.json")),
    ]
}

pub fn load_config() -> Result<Config> {
    let path = Config::default_path()?;
    Config::load_or_default(&path)
}

/// Resolve which model would handle this run.
pub async fn resolve_routing(config: &Config) -> Routing {
    ModelRouter::new(config.ai.clone()).resolve().await
}

/// `outlaw models` -- show what the router would choose, and why.
pub async fn show_models(json: bool) -> Result<()> {
    let config = load_config()?;
    let routing = resolve_routing(&config).await;

    let platform = ork_core::platform::detect()?;
    let gpus = platform.gpus()?;
    let advice = advise_for_vram(&gpus);
    let library = RunbookLibrary::load(runbook_dir().as_deref())?;

    if json {
        let attempts: Vec<_> = routing
            .attempts
            .iter()
            .map(|attempt| {
                serde_json::json!({
                    "tier": attempt.tier.as_str(),
                    "outcome": attempt.outcome.explain(),
                    "selected": attempt.outcome.is_selected(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "selected": routing.tier.map(ModelTier::as_str),
                "summary": routing.summary(),
                "attempts": attempts,
                "gpus": gpus,
                "vram_recommendation": advice.recommendation,
                "runbook_entries": library.len(),
                "cloud_key_stored": secrets::is_set(SecretKind::CloudApiKey),
            }))?
        );
        return Ok(());
    }

    println!("{}", bold("Model routing"));
    println!("  {}", routing.summary());
    println!();

    println!("{}", bold("How that was decided"));
    for attempt in &routing.attempts {
        let marker = if attempt.outcome.is_selected() {
            "->"
        } else {
            "  "
        };
        println!(
            "  {marker} {:<8} {}",
            attempt.tier.as_str(),
            dim(&attempt.outcome.explain())
        );
    }
    println!();

    println!("{}", bold("Graphics hardware"));
    if gpus.is_empty() {
        println!("  {}", dim("none detected"));
    }
    for gpu in &gpus {
        let vram = gpu
            .vram_total_bytes
            .map(ork_core::util::format_bytes)
            .unwrap_or_else(|| "unknown".to_string());
        println!("  {} -- {vram} of video memory", gpu.name);
    }
    for line in wrap(&advice.recommendation, 72) {
        println!("  {}", dim(&line));
    }
    println!();

    println!("{}", bold("Runbook library"));
    println!("  {} entries available without a model", library.len());
    if !secrets::is_set(SecretKind::CloudApiKey) && config.ai.cloud.enabled {
        println!();
        println!(
            "  {}",
            dim("The cloud tier is enabled but no API key is stored. Run `outlaw set-key`.")
        );
    }
    Ok(())
}

/// `outlaw config` -- show where settings live and what they currently say.
pub fn show_config(json: bool) -> Result<()> {
    let path = Config::default_path()?;
    let config = Config::load_or_default(&path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": path.display().to_string(),
                "exists": path.exists(),
                "runbook_dir": runbook_dir().map(|d| d.display().to_string()),
                "paths": where_things_live()
                    .into_iter()
                    .map(|(what, path)| serde_json::json!({
                        "what": what,
                        "path": path.display().to_string(),
                        "exists": path.exists(),
                    }))
                    .collect::<Vec<_>>(),
                "config": config,
                "cloud_key_stored": secrets::is_set(SecretKind::CloudApiKey),
                "remote_token_stored": secrets::is_set(SecretKind::RemoteEndpointToken),
            }))?
        );
        return Ok(());
    }

    println!("{}", bold("Where your data lives"));
    for (what, place) in where_things_live() {
        // Saying which of these exist matters: "not created yet" is the normal
        // state for most of them on a fresh install, and somebody who cannot
        // find a file they were told about should be able to see that the tool
        // has not written it rather than wonder where it went.
        let note = if place.exists() {
            String::new()
        } else {
            dim("  (not created yet)")
        };
        println!("  {what:<26}{}{note}", place.display());
    }
    println!();
    println!(
        "  {}",
        dim(
            "Nothing is written outside these. Deleting any of them is safe; the tool starts over."
        )
    );
    println!();

    println!("{}", bold("Current settings"));
    for line in config.to_toml()?.lines() {
        println!("  {line}");
    }
    println!();

    println!("{}", bold("Stored credentials"));
    for kind in [SecretKind::CloudApiKey, SecretKind::RemoteEndpointToken] {
        let state = if secrets::is_set(kind) {
            "stored"
        } else {
            "not set"
        };
        println!("  {:<28}{}", kind.label(), dim(state));
    }
    println!();
    println!(
        "  {}",
        dim("Keys are kept in the operating system's credential store, never in the file above.")
    );
    Ok(())
}

/// `outlaw set-key` -- store a credential, read from standard input.
///
/// Read from stdin rather than taken as an argument, because a secret passed
/// on the command line ends up in shell history and in the process list.
pub fn set_key(which: &str) -> Result<()> {
    let kind = match which {
        "cloud" => SecretKind::CloudApiKey,
        "remote" => SecretKind::RemoteEndpointToken,
        other => {
            crate::refusal::refuse!("unknown credential `{other}` (expected `cloud` or `remote`)")
        }
    };

    eprintln!("Paste the {} and press Enter:", kind.label());
    let mut value = String::new();
    std::io::stdin()
        .read_line(&mut value)
        .context("could not read from standard input")?;

    secrets::set(kind, value.trim())?;
    eprintln!("Saved to the system credential store.");
    Ok(())
}

/// `outlaw set-key --remove`
pub fn clear_key(which: &str) -> Result<()> {
    let kind = match which {
        "cloud" => SecretKind::CloudApiKey,
        "remote" => SecretKind::RemoteEndpointToken,
        other => {
            crate::refusal::refuse!("unknown credential `{other}` (expected `cloud` or `remote`)")
        }
    };
    secrets::delete(kind)?;
    eprintln!("Removed the {} from the credential store.", kind.label());
    Ok(())
}

fn invasiveness_label(invasiveness: Invasiveness) -> &'static str {
    match invasiveness {
        Invasiveness::Inspect => "look",
        Invasiveness::Low => "minor",
        Invasiveness::Medium => "moderate",
        Invasiveness::High => "major",
    }
}

/// Explain a scan report, printing the result.
pub async fn explain(report: &ScanReport, json: bool) -> Result<()> {
    let config = load_config()?;
    let routing = resolve_routing(&config).await;
    let library = RunbookLibrary::load(runbook_dir().as_deref())?;
    let platform = ork_core::platform::detect()?.kind().to_string();

    let analysis = Analyst::new(library, platform)
        .analyse(report, &routing)
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
        return Ok(());
    }

    if analysis.is_empty() {
        println!("Nothing to explain -- the scan found no problems.");
        return Ok(());
    }

    println!();
    println!("{}", bold("What this means"));
    println!("  {}", dim(&routing.summary()));
    println!(
        "  {}",
        dim(&format!(
            "{} of {} answered from the runbook library",
            analysis.answered_by_runbook,
            analysis.items.len()
        ))
    );
    println!();

    if let Some(correlation) = &analysis.correlation {
        println!("{}", bold("Taken together"));
        for line in wrap(correlation, 74) {
            println!("  {line}");
        }
        println!();
    }

    for item in &analysis.items {
        println!("{}", bold(&item.title));
        let provenance = match &item.source {
            AnalysisSource::Runbook { entry_id } => format!("known problem: {entry_id}"),
            AnalysisSource::Model { model } => format!("reasoned by {model}"),
            AnalysisSource::Unexplained => "no known answer".to_string(),
        };
        println!("  {}", dim(&provenance));

        for line in wrap(&item.explanation, 74) {
            println!("  {line}");
        }

        if !item.fixes.is_empty() {
            println!();
            println!("  {}", dim("Things to try, least disruptive first:"));
            for (index, fix) in item.fixes.iter().enumerate() {
                let label = invasiveness_label(fix.invasiveness);
                println!("   {}. [{label}] {}", index + 1, fix.description);
                if let Some(command) = &fix.command {
                    println!("      {}", dim(command));
                }
            }
        }
        println!();
    }

    if analysis.unexplained() > 0 {
        println!(
            "{}",
            dim(&format!(
                "{} no explanation. Configure a model with `outlaw models` \
                 to have them reasoned about.",
                ork_core::util::counted_as(analysis.unexplained(), "finding has", "findings have")
            ))
        );
    }
    Ok(())
}
