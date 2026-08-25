//! Turning core types into something a person can read.

use anyhow::Result;
use ork_core::probe::ProbeStatus;
use ork_core::scan::{ScanEvent, ScanReport};
use ork_core::util::format_bytes;

use crate::style::{bold, dim, severity_label};

/// One line per probe as it finishes, written to stderr.
pub fn progress(event: &ScanEvent) {
    match event {
        ScanEvent::Started { tier, probe_count } => {
            eprintln!(
                "{}",
                bold(&format!("Running a {tier} scan ({probe_count} checks)"))
            );
        }
        ScanEvent::ProbeFinished { outcome } => {
            let marker = match &outcome.status {
                ProbeStatus::Completed if outcome.findings.is_empty() => "ok  ".to_string(),
                ProbeStatus::Completed => format!("{:<4}", outcome.findings.len()),
                ProbeStatus::Skipped(_) => "skip".to_string(),
                ProbeStatus::Failed { .. } => "fail".to_string(),
                ProbeStatus::Cancelled => "stop".to_string(),
            };
            let note = match &outcome.status {
                ProbeStatus::Skipped(reason) => format!(" ({reason})"),
                ProbeStatus::Failed { error } => format!(" ({error})"),
                _ => String::new(),
            };
            eprintln!("  {} {}{}", dim(&marker), outcome.name, dim(&note));
        }
        ScanEvent::Finished { .. } | ScanEvent::ProbeStarted { .. } => {}
    }
}

/// The full human-readable report.
pub fn report(report: &ScanReport) {
    let findings = report.findings();

    println!();
    println!(
        "{}",
        bold(&format!(
            "{} -- {} scan, {:.1}s",
            report.host.hostname,
            report.tier,
            report.duration.as_secs_f64()
        ))
    );
    println!(
        "{}",
        dim(&format!("{} ({})", report.host.os_name, report.host.arch))
    );
    println!();

    if report.cancelled {
        println!(
            "{}",
            bold("Scan was stopped early -- these results are incomplete.")
        );
        println!();
    }

    if findings.is_empty() {
        println!("No problems found.");
    } else {
        println!("{}", bold(&format!("{} finding(s)", findings.len())));
        println!();
        for finding in &findings {
            println!(
                "{}  {}",
                severity_label(finding.severity),
                bold(&finding.title)
            );
            for line in wrap(&finding.detail, 76) {
                println!("            {line}");
            }
            if let Some(hint) = &finding.remediation_hint {
                for (index, line) in wrap(hint, 72).into_iter().enumerate() {
                    let prefix = if index == 0 { "Fix:  " } else { "      " };
                    println!("            {}", dim(&format!("{prefix}{line}")));
                }
            }
            println!();
        }
    }

    let skipped: Vec<_> = report.skipped().collect();
    if !skipped.is_empty() {
        println!(
            "{}",
            bold(&format!("{} check(s) did not run", skipped.len()))
        );
        for outcome in skipped {
            if let ProbeStatus::Skipped(reason) = &outcome.status {
                println!("  {} {}", outcome.name, dim(&format!("-- {reason}")));
            }
        }
        println!();
    }

    let failed: Vec<_> = report.failed().collect();
    if !failed.is_empty() {
        println!(
            "{}",
            bold(&format!("{} check(s) failed to run", failed.len()))
        );
        for outcome in failed {
            if let ProbeStatus::Failed { error } = &outcome.status {
                println!("  {} {}", outcome.name, dim(&format!("-- {error}")));
            }
        }
        println!();
    }
}

/// List the checks this build knows how to run.
pub fn probes(json: bool) -> Result<()> {
    let metas = ork_core::probes::all_meta();

    if json {
        let value: Vec<_> = metas
            .iter()
            .map(|meta| {
                serde_json::json!({
                    "id": meta.id,
                    "name": meta.name,
                    "description": meta.description,
                    "category": meta.category.as_str(),
                    "min_tier": meta.min_tier.as_str(),
                    "platforms": meta.platforms.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                    "requires_tools": meta.requires_tools,
                    "requires_elevation": meta.requires_elevation,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("{}", bold(&format!("{} check(s) available", metas.len())));
    println!();
    for meta in metas {
        let platforms: Vec<&str> = meta.platforms.iter().map(|p| p.as_str()).collect();
        println!(
            "{}  {}",
            bold(meta.id),
            dim(&format!("[{} scan]", meta.min_tier))
        );
        println!("  {}", meta.description);
        println!("  {}", dim(&format!("runs on: {}", platforms.join(", "))));
        if !meta.requires_tools.is_empty() {
            println!(
                "  {}",
                dim(&format!("needs: {}", meta.requires_tools.join(", ")))
            );
        }
        if meta.requires_elevation {
            println!("  {}", dim("needs administrator rights"));
        }
        println!();
    }
    Ok(())
}

/// Show what the platform layer detected about this machine.
pub fn host(json: bool) -> Result<()> {
    let platform = ork_core::platform::detect()?;
    let host = platform.host()?;
    let volumes = platform.volumes()?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "platform": platform.kind().as_str(),
                "host": host,
                "volumes": volumes,
            }))?
        );
        return Ok(());
    }

    println!("{}", bold(&host.hostname));
    println!("  {:<16}{}", "platform", platform.kind());
    println!("  {:<16}{} ({})", "os", host.os_name, host.os_version);
    println!("  {:<16}{}", "kernel", host.kernel_version);
    println!("  {:<16}{}", "arch", host.arch);
    println!("  {:<16}{}", "cpu", host.cpu_brand);
    let cores = match host.physical_cores {
        Some(physical) => format!("{physical} physical / {} logical", host.logical_cores),
        None => format!("{} logical", host.logical_cores),
    };
    println!("  {:<16}{cores}", "cores");
    println!(
        "  {:<16}{}",
        "memory",
        format_bytes(host.total_memory_bytes)
    );
    println!();

    println!("{}", bold(&format!("{} volume(s)", volumes.len())));
    for volume in &volumes {
        let free_percent = volume.free_fraction().unwrap_or(0.0) * 100.0;
        println!(
            "  {:<12} {:>10} free of {:>10}  ({free_percent:.1}%)  {}  {}",
            volume.mount_point,
            format_bytes(volume.available_bytes),
            format_bytes(volume.total_bytes),
            volume.filesystem,
            dim(&format!("{:?}", volume.role)),
        );
    }
    Ok(())
}

/// Wrap text to `width` columns on word boundaries.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wraps_on_word_boundaries() {
        let lines = wrap("the quick brown fox jumps over the lazy dog", 15);
        assert!(lines.iter().all(|line| line.len() <= 15), "got {lines:?}");
        assert_eq!(
            lines.join(" "),
            "the quick brown fox jumps over the lazy dog"
        );
    }

    #[test]
    fn empty_text_produces_no_lines() {
        assert!(wrap("", 20).is_empty());
        assert!(wrap("   \n  ", 20).is_empty());
    }

    #[test]
    fn a_word_longer_than_the_width_is_not_lost() {
        // Log messages contain paths and identifiers longer than any sensible
        // wrap width. Dropping or panicking on them would lose the evidence.
        let long = "a".repeat(200);
        let lines = wrap(&long, 20);
        assert_eq!(lines, vec![long]);
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        // Log messages arrive in whatever language the system is set to.
        let lines = wrap("ошибка драйвера видеокарты произошла снова", 20);
        assert!(!lines.is_empty());
        assert_eq!(
            lines.join(" "),
            "ошибка драйвера видеокарты произошла снова"
        );
    }

    #[test]
    fn newlines_in_source_text_are_normalised_away() {
        let lines = wrap("first line\nsecond line", 100);
        assert_eq!(lines, vec!["first line second line"]);
    }
}
