//! Asking a drive what it thinks of itself.
//!
//! A failing disk is the most consequential thing this tool can find, and the
//! only one where noticing a week early is the difference between an
//! inconvenience and losing everything. It is also the easiest thing to get
//! wrong, because SMART data is famously over-interpreted: whole articles have
//! been written declaring drives doomed on the strength of a raw attribute
//! value that meant something else entirely on that manufacturer's firmware.
//!
//! So the rule here is: **the drive's own overall verdict is the finding.**
//! Windows and SMART both publish a single pass/fail self-assessment, computed
//! by the firmware against thresholds the manufacturer set. That is the number
//! reported. Individual attributes -- reallocated sectors, media errors, wear
//! -- are carried as *evidence beside* that verdict, never as the basis for
//! one. This tool does not invent thresholds it cannot defend.
//!
//! One thing is deliberately never collected: **serial numbers.** They
//! identify a specific piece of hardware, they are of no diagnostic use here,
//! and a bug report should not carry one.

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::platform::common::run_capture;

/// What the drive says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "kebab-case")]
pub enum DriveVerdict {
    /// The drive's own self-assessment passed.
    Healthy,
    /// It is working, but reporting something wrong.
    Warning { detail: String },
    /// The drive expects to fail. This is the one that matters.
    Failing { detail: String },
    /// No usable answer. Never read as either of the above.
    Unknown { detail: String },
}

impl DriveVerdict {
    pub fn is_healthy(&self) -> bool {
        matches!(self, DriveVerdict::Healthy)
    }
}

/// One drive, and what could be learned about it.
///
/// Every measurement is optional because which ones exist depends on the
/// drive, the interface, and whether the scan was elevated. An absent value
/// means "not reported", never "zero".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveHealth {
    /// How the operating system refers to it: `/dev/sda`, or a disk number.
    pub name: String,
    /// The model, as the drive reports it. No serial number.
    pub model: String,
    /// `ssd`, `hdd`, `nvme`, or empty when it does not say.
    pub kind: String,
    pub verdict: DriveVerdict,
    pub temperature_c: Option<i64>,
    pub power_on_hours: Option<i64>,
    /// Sectors the drive has retired because they went bad. Zero is normal;
    /// a number that grows between scans is not.
    pub reallocated_sectors: Option<i64>,
    /// Unrecoverable errors, on drives that count them.
    pub media_errors: Option<i64>,
    /// How much of the drive's rated write endurance is used up, as a
    /// percentage. SSDs and NVMe drives only.
    pub wear_percent: Option<i64>,
}

impl Default for DriveVerdict {
    fn default() -> Self {
        DriveVerdict::Unknown {
            detail: "nothing was reported".to_string(),
        }
    }
}

impl DriveHealth {
    /// How to refer to this drive in a sentence.
    pub fn describe(&self) -> String {
        match (self.model.trim().is_empty(), self.name.trim().is_empty()) {
            (false, false) => format!("{} ({})", self.model, self.name),
            (false, true) => self.model.clone(),
            (true, false) => self.name.clone(),
            (true, true) => "an unnamed drive".to_string(),
        }
    }
}

/// Ask Windows, through the storage subsystem.
///
/// `Get-PhysicalDisk` needs no elevation and reports the health status the
/// drive itself published. The reliability counters that carry temperature and
/// wear *do* need elevation, which is why they are a separate probe rather
/// than a silent absence here.
#[cfg(windows)]
pub fn health() -> Result<Vec<DriveHealth>> {
    // A pipe, not a comma: model names contain commas often enough to matter,
    // and a split that loses half a drive's name is worse than no name.
    let script = "Get-PhysicalDisk | ForEach-Object { \
         \"$($_.DeviceId)|$($_.FriendlyName)|$($_.MediaType)|$($_.HealthStatus)|$($_.OperationalStatus)\" }";

    let output = run_capture(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?;
    Ok(parse_windows(&output.stdout))
}

/// Ask the drives directly, through smartmontools.
#[cfg(target_os = "linux")]
pub fn health() -> Result<Vec<DriveHealth>> {
    let scan = run_capture("smartctl", &["--scan", "--json"])?;
    let mut drives = Vec::new();

    for device in devices_from_scan(&scan.stdout) {
        // smartctl's exit code is a bitmask, not a success flag: a drive that
        // is failing its self-assessment sets a bit and exits non-zero, which
        // is precisely the case worth reporting. Treating that as a failed
        // command would silently discard the most important result.
        let Ok(output) = run_capture("smartctl", &["--json", "-a", &device]) else {
            drives.push(DriveHealth {
                name: device.clone(),
                verdict: DriveVerdict::Unknown {
                    detail: "smartctl could not be run".to_string(),
                },
                ..Default::default()
            });
            continue;
        };

        match parse_smartctl(&output.stdout) {
            Some(mut drive) => {
                if drive.name.trim().is_empty() {
                    drive.name = device.clone();
                }
                drives.push(drive);
            }
            None => drives.push(DriveHealth {
                name: device.clone(),
                verdict: DriveVerdict::Unknown {
                    detail: "smartctl gave no usable answer".to_string(),
                },
                ..Default::default()
            }),
        }
    }

    Ok(drives)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn health() -> Result<Vec<DriveHealth>> {
    Ok(Vec::new())
}

/// Turn `Get-PhysicalDisk` output into drives.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_windows(output: &str) -> Vec<DriveHealth> {
    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            if fields.len() < 5 {
                return None;
            }
            let name = fields[0].trim();
            if name.is_empty() {
                return None;
            }

            Some(DriveHealth {
                name: format!("disk {name}"),
                model: fields[1].trim().to_string(),
                kind: normalise_kind(fields[2].trim()),
                verdict: interpret_windows(fields[3].trim(), fields[4].trim()),
                ..Default::default()
            })
        })
        .collect()
}

/// Windows reports two things, and they disagree in useful ways.
///
/// `HealthStatus` is the drive's own verdict. `OperationalStatus` is whether
/// the system can currently use it -- a drive that is perfectly healthy but
/// has been taken offline reports `Healthy` and `Offline`, and neither of
/// those alone is the whole story.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn interpret_windows(health: &str, operational: &str) -> DriveVerdict {
    let operational_note = {
        let said = operational.trim().to_ascii_lowercase();
        // "OK" and "Online" both mean nothing to report.
        (!said.is_empty() && said != "ok" && said != "online").then(|| operational.trim())
    };

    match health.trim().to_ascii_lowercase().as_str() {
        "healthy" => match operational_note {
            None => DriveVerdict::Healthy,
            // Healthy but not usable. Worth saying, and not a hardware fault.
            Some(note) => DriveVerdict::Warning {
                detail: format!("the drive reports itself healthy but is {note}"),
            },
        },
        "warning" => DriveVerdict::Warning {
            detail: match operational_note {
                Some(note) => format!("the drive is reporting a problem, and is {note}"),
                None => "the drive is reporting a problem".to_string(),
            },
        },
        "unhealthy" => DriveVerdict::Failing {
            detail: match operational_note {
                Some(note) => format!("the drive reports itself unhealthy, and is {note}"),
                None => "the drive reports itself unhealthy".to_string(),
            },
        },
        // Includes the literal "Unknown" that Windows gives for drives behind
        // a controller it cannot query -- USB enclosures, most often.
        other => DriveVerdict::Unknown {
            detail: if other.is_empty() {
                "Windows reported no health status".to_string()
            } else {
                format!("Windows reported the health status as {}", health.trim())
            },
        },
    }
}

fn normalise_kind(media: &str) -> String {
    match media.trim().to_ascii_lowercase().as_str() {
        "ssd" => "ssd".to_string(),
        "hdd" => "hdd".to_string(),
        "scm" | "nvme" => "nvme".to_string(),
        // Windows says "Unspecified" for anything behind a USB bridge, which
        // is not a kind of drive.
        _ => String::new(),
    }
}

/// Device names from `smartctl --scan --json`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn devices_from_scan(json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value
        .get("devices")
        .and_then(|devices| devices.as_array())
        .map(|devices| {
            devices
                .iter()
                .filter_map(|device| device.get("name")?.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Read one drive out of `smartctl --json -a`.
///
/// `smart_status.passed` is the firmware's own overall self-assessment,
/// computed against thresholds the manufacturer set. It is the verdict. The
/// attributes read afterwards are evidence beside it and never override it --
/// a non-zero reallocated-sector count on a drive that passes its own
/// assessment is worth mentioning, not worth calling a failure.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_smartctl(json: &str) -> Option<DriveHealth> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;

    let string_at = |path: &[&str]| -> String {
        let mut node = &value;
        for key in path {
            match node.get(key) {
                Some(next) => node = next,
                None => return String::new(),
            }
        }
        node.as_str().unwrap_or_default().to_string()
    };
    let number_at = |path: &[&str]| -> Option<i64> {
        let mut node = &value;
        for key in path {
            node = node.get(key)?;
        }
        node.as_i64()
    };

    let verdict = match value
        .get("smart_status")
        .and_then(|status| status.get("passed"))
        .and_then(serde_json::Value::as_bool)
    {
        Some(true) => DriveVerdict::Healthy,
        Some(false) => DriveVerdict::Failing {
            detail: "the drive failed its own self-assessment".to_string(),
        },
        // Common and not alarming: many drives behind a USB bridge answer
        // everything else and refuse this. Not knowing is not bad news.
        None => DriveVerdict::Unknown {
            detail: "the drive did not report a self-assessment".to_string(),
        },
    };

    // NVMe keeps its temperature in tenths of a kelvin in one place and plain
    // Celsius in another; the plain one is preferred where it exists.
    let temperature_c = number_at(&["temperature", "current"]);

    Some(DriveHealth {
        name: string_at(&["device", "name"]),
        model: string_at(&["model_name"]),
        kind: smartctl_kind(&value),
        verdict,
        temperature_c,
        power_on_hours: number_at(&["power_on_time", "hours"]),
        reallocated_sectors: ata_attribute(&value, 5),
        media_errors: number_at(&["nvme_smart_health_information_log", "media_errors"]),
        wear_percent: number_at(&["nvme_smart_health_information_log", "percentage_used"])
            .or_else(|| ata_attribute(&value, 177))
            .or_else(|| ata_attribute(&value, 231)),
    })
}

fn smartctl_kind(value: &serde_json::Value) -> String {
    if value.get("nvme_smart_health_information_log").is_some() {
        return "nvme".to_string();
    }
    match value
        .get("rotation_rate")
        .and_then(serde_json::Value::as_i64)
    {
        // smartctl reports zero for anything with no platters.
        Some(0) => "ssd".to_string(),
        Some(_) => "hdd".to_string(),
        None => String::new(),
    }
}

/// The raw value of one numbered ATA SMART attribute.
///
/// By number rather than by name: manufacturers spell the names differently
/// (`Reallocated_Sector_Ct`, `Reallocated_Sector_Count`) but the numbers are
/// fixed by the specification.
fn ata_attribute(value: &serde_json::Value, id: i64) -> Option<i64> {
    value
        .get("ata_smart_attributes")?
        .get("table")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(serde_json::Value::as_i64) == Some(id))?
        .get("raw")?
        .get("value")?
        .as_i64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healthy_windows_drive_is_healthy() {
        let drives = parse_windows("0|Samsung SSD 850 PRO 1TB|SSD|Healthy|OK\n");
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].model, "Samsung SSD 850 PRO 1TB");
        assert_eq!(drives[0].kind, "ssd");
        assert_eq!(drives[0].verdict, DriveVerdict::Healthy);
    }

    #[test]
    fn a_model_name_containing_a_comma_survives() {
        // Which is why the fields are pipe-separated. A drive called
        // "WDC WD40, Blue" would otherwise lose half its name.
        let drives = parse_windows("1|WDC WD40, Blue|HDD|Healthy|OK\n");
        assert_eq!(drives[0].model, "WDC WD40, Blue");
    }

    #[test]
    fn an_unhealthy_drive_is_reported_as_failing() {
        let drives = parse_windows("1|Some Drive|HDD|Unhealthy|OK\n");
        assert!(matches!(drives[0].verdict, DriveVerdict::Failing { .. }));
    }

    #[test]
    fn a_healthy_drive_that_is_offline_is_not_silently_called_fine() {
        // Two different questions. A drive can be in perfect condition and
        // still be unusable, and saying only "healthy" would hide that.
        let drives = parse_windows("1|Some Drive|HDD|Healthy|Offline\n");
        match &drives[0].verdict {
            DriveVerdict::Warning { detail } => assert!(detail.contains("Offline")),
            other => panic!("expected a warning, got {other:?}"),
        }
    }

    #[test]
    fn a_drive_windows_cannot_query_is_unknown_rather_than_healthy() {
        // Most external enclosures land here. Not knowing is not good news,
        // and it is certainly not bad news either.
        let drives = parse_windows("2|Seagate Expansion|Unspecified|Unknown|OK\n");
        assert!(matches!(drives[0].verdict, DriveVerdict::Unknown { .. }));
        assert_eq!(drives[0].kind, "", "a USB bridge is not a kind of drive");
    }

    #[test]
    fn malformed_and_blank_lines_are_skipped_rather_than_reported() {
        let drives =
            parse_windows("\n   \nnot-enough-fields\n0|A|SSD|Healthy|OK\n|B|SSD|Healthy|OK\n");
        assert_eq!(drives.len(), 1, "got {drives:?}");
        assert_eq!(drives[0].model, "A");
    }

    #[test]
    fn a_passing_smart_assessment_is_healthy() {
        let drive = parse_smartctl(
            r#"{
                "device": {"name": "/dev/sda"},
                "model_name": "Samsung SSD 860",
                "rotation_rate": 0,
                "smart_status": {"passed": true},
                "temperature": {"current": 34},
                "power_on_time": {"hours": 12045}
            }"#,
        )
        .expect("valid smartctl output");

        assert_eq!(drive.verdict, DriveVerdict::Healthy);
        assert_eq!(drive.name, "/dev/sda");
        assert_eq!(drive.kind, "ssd");
        assert_eq!(drive.temperature_c, Some(34));
        assert_eq!(drive.power_on_hours, Some(12045));
    }

    #[test]
    fn a_failed_smart_assessment_is_the_finding() {
        let drive =
            parse_smartctl(r#"{"device":{"name":"/dev/sdb"},"smart_status":{"passed":false}}"#)
                .expect("valid smartctl output");
        assert!(matches!(drive.verdict, DriveVerdict::Failing { .. }));
    }

    #[test]
    fn a_drive_that_will_not_answer_is_unknown_not_healthy() {
        // Many USB enclosures answer everything except this. Reading a
        // missing verdict as a passing one would be the worst possible
        // mistake this file could make.
        let drive = parse_smartctl(r#"{"device":{"name":"/dev/sdc"},"model_name":"X"}"#)
            .expect("valid smartctl output");
        assert!(matches!(drive.verdict, DriveVerdict::Unknown { .. }));
    }

    #[test]
    fn worn_attributes_do_not_override_a_passing_assessment() {
        // This is the discipline the whole module rests on. A non-zero
        // reallocated count is worth mentioning; it is not this tool's place
        // to overrule the firmware's own thresholds with an invented one.
        let drive = parse_smartctl(
            r#"{
                "device": {"name": "/dev/sda"},
                "rotation_rate": 7200,
                "smart_status": {"passed": true},
                "ata_smart_attributes": {"table": [
                    {"id": 5, "name": "Reallocated_Sector_Ct", "raw": {"value": 48}},
                    {"id": 231, "name": "SSD_Life_Left", "raw": {"value": 91}}
                ]}
            }"#,
        )
        .expect("valid smartctl output");

        assert_eq!(drive.verdict, DriveVerdict::Healthy);
        assert_eq!(drive.reallocated_sectors, Some(48));
        assert_eq!(drive.kind, "hdd");
    }

    #[test]
    fn nvme_wear_and_errors_are_read_from_their_own_log() {
        let drive = parse_smartctl(
            r#"{
                "device": {"name": "/dev/nvme0"},
                "model_name": "WD Black SN850X",
                "smart_status": {"passed": true},
                "temperature": {"current": 41},
                "nvme_smart_health_information_log": {
                    "critical_warning": 0,
                    "percentage_used": 7,
                    "media_errors": 0
                }
            }"#,
        )
        .expect("valid smartctl output");

        assert_eq!(drive.kind, "nvme");
        assert_eq!(drive.wear_percent, Some(7));
        assert_eq!(drive.media_errors, Some(0));
    }

    #[test]
    fn a_serial_number_is_never_carried_out_of_here() {
        // It identifies a specific piece of hardware, it is of no diagnostic
        // use, and it must not end up in a bug report.
        let drive = parse_smartctl(
            r#"{
                "device": {"name": "/dev/sda"},
                "model_name": "Samsung SSD 860",
                "serial_number": "S3Z1NB0K123456",
                "smart_status": {"passed": true}
            }"#,
        )
        .expect("valid smartctl output");

        let rendered = serde_json::to_string(&drive).unwrap();
        assert!(
            !rendered.contains("S3Z1NB0K123456"),
            "a serial number escaped: {rendered}"
        );
    }

    #[test]
    fn nonsense_input_produces_nothing_rather_than_a_guess() {
        assert!(parse_smartctl("not json").is_none());
        assert!(parse_smartctl("").is_none());
        assert!(parse_windows("").is_empty());
        assert!(devices_from_scan("not json").is_empty());
        assert!(devices_from_scan(r#"{"devices":[]}"#).is_empty());
    }

    #[test]
    fn devices_are_read_from_a_scan() {
        let devices = devices_from_scan(
            r#"{"devices":[{"name":"/dev/sda","type":"sat"},{"name":"/dev/nvme0","type":"nvme"}]}"#,
        );
        assert_eq!(devices, vec!["/dev/sda", "/dev/nvme0"]);
    }

    /// Asks this machine's real drives. The words above are only worth
    /// anything if they are the words this computer actually says.
    #[test]
    fn the_real_drives_on_this_machine_give_answers_that_parse() {
        let Ok(drives) = health() else {
            return;
        };
        for drive in &drives {
            assert!(
                !drive.describe().is_empty(),
                "a drive with no way to refer to it: {drive:?}"
            );
        }
    }
}
