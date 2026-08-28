//! Watching the temperature while the machine is being worked hard.
//!
//! A burn-in test that is not watching the temperature is not a test, it is a
//! way of breaking somebody's laptop. Everything else in this tool observes; a
//! stress test is the one thing here that deliberately makes the machine do
//! something, and the thing it makes the machine do is get hot.
//!
//! So this runs alongside the workers and can stop them. That is not a
//! timeout -- nothing here is ever stopped for taking too long -- it is a
//! hardware protection stop, and when it fires it is reported loudly, because
//! "this machine cannot be worked hard without overheating" is the single most
//! useful thing a burn-in can discover.

use serde::{Deserialize, Serialize};
use sysinfo::Components;

/// The ceiling to use when the machine does not report one of its own.
///
/// Deliberately conservative. Silicon that throttles at 100C is common and
/// perfectly healthy, so this is not a claim about damage -- it is the point
/// past which we would rather stop and say so than keep pushing a machine we
/// know nothing about.
pub const DEFAULT_CEILING_C: f32 = 95.0;

/// How far below a manufacturer's stated critical temperature to stop.
///
/// Stopping *at* critical means the reading that stops us is the reading that
/// was already too high.
pub const MARGIN_C: f32 = 3.0;

/// Readings outside this range are treated as no reading at all.
///
/// Machines are full of sensors that report a constant 0, a constant 128, or
/// the temperature of something that is not there. Believing one of those
/// would either abort every run on a healthy machine or, worse, sit quietly at
/// "0C" while the processor cooks.
fn plausible(celsius: f32) -> bool {
    (1.0..=150.0).contains(&celsius)
}

/// The hottest a named part of the machine got during a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Heat {
    /// What the machine calls this sensor. Not standardised, and often not
    /// pretty -- shown as-is rather than guessed at.
    pub label: String,
    pub peak_c: f32,
    /// The temperature this machine says is critical for this part, when it
    /// says.
    pub critical_c: Option<f32>,
}

impl Heat {
    /// The temperature at which this sensor should stop the run.
    pub fn ceiling(&self) -> f32 {
        match self.critical_c {
            Some(critical) if plausible(critical) => critical - MARGIN_C,
            _ => DEFAULT_CEILING_C,
        }
    }
}

/// One reading from one sensor.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub label: String,
    pub celsius: f32,
    pub critical_c: Option<f32>,
}

/// Sensors, and the hottest each has been.
///
/// Kept as a list rather than a single number because "the machine reached
/// 97C" is not actionable and "the GPU reached 97C while the processor stayed
/// at 61C" is.
// Deliberately not `Default`. A default thermometer would have to be a blind
// one, and `unwrap_or_default()` on an `Option<Thermometer>` would then read
// as harmless while quietly turning off the only thing watching how hot the
// machine gets. Clippy suggests exactly that substitution, which is how this
// was noticed.
#[derive(Debug)]
pub struct Thermometer {
    source: Source,
    peaks: Vec<Heat>,
}

/// Where readings come from.
#[derive(Debug)]
enum Source {
    Sensors(Box<Components>),
    /// Nothing to read. Also what a machine without usable sensors amounts to.
    Blind,
    /// Readings decided in advance.
    ///
    /// The only way to test what happens when a machine overheats without
    /// overheating a machine. The stop-on-heat path is the one rail here that
    /// protects hardware, and leaving it exercised only by whatever this
    /// developer's computer happened to do would mean shipping it never having
    /// once seen it fire.
    #[cfg(test)]
    Scripted(std::collections::VecDeque<Vec<Reading>>),
}

impl Thermometer {
    /// Read this machine's own sensors.
    pub fn sensors() -> Self {
        Self {
            source: Source::Sensors(Box::new(Components::new_with_refreshed_list())),
            peaks: Vec::new(),
        }
    }

    /// A thermometer that will never read anything, for tests and for
    /// machines where sensors are not reachable.
    pub fn blind() -> Self {
        Self {
            source: Source::Blind,
            peaks: Vec::new(),
        }
    }

    /// A thermometer that will report exactly these readings, in order, and
    /// then repeat the last one.
    #[cfg(test)]
    pub fn scripted(readings: Vec<Vec<Reading>>) -> Self {
        Self {
            source: Source::Scripted(readings.into()),
            peaks: Vec::new(),
        }
    }

    /// Take a reading from every sensor that is telling the truth.
    pub fn read(&mut self) -> Vec<Reading> {
        #[cfg(test)]
        if let Source::Scripted(queue) = &mut self.source {
            let readings = if queue.len() > 1 {
                queue.pop_front().unwrap_or_default()
            } else {
                queue.front().cloned().unwrap_or_default()
            };
            for reading in &readings {
                self.record(reading);
            }
            return readings;
        }

        let Source::Sensors(components) = &mut self.source else {
            return Vec::new();
        };
        components.refresh(false);
        let readings: Vec<Reading> = components
            .iter()
            .filter_map(|component| {
                let celsius = component.temperature()?;
                if !plausible(celsius) {
                    return None;
                }
                Some(Reading {
                    label: component.label().to_string(),
                    celsius,
                    critical_c: component.critical(),
                })
            })
            .collect();
        for reading in &readings {
            self.record(reading);
        }
        readings
    }

    /// Fold a reading into the running peaks. Public so the recording rule can
    /// be tested without a machine that has sensors.
    pub fn record(&mut self, reading: &Reading) {
        match self
            .peaks
            .iter_mut()
            .find(|heat| heat.label == reading.label)
        {
            Some(heat) => {
                if reading.celsius > heat.peak_c {
                    heat.peak_c = reading.celsius;
                }
                // A machine that reports its critical temperature late is more
                // common than one that never does, and a stated ceiling always
                // beats our guess.
                if heat.critical_c.is_none() {
                    heat.critical_c = reading.critical_c;
                }
            }
            None => self.peaks.push(Heat {
                label: reading.label.clone(),
                peak_c: reading.celsius,
                critical_c: reading.critical_c,
            }),
        }
    }

    /// Whether anything at all could be read.
    ///
    /// A run where this is false is a run where nothing was watching the
    /// temperature, and the report has to say so rather than leaving an empty
    /// list to be read as "it never got hot".
    pub fn saw_anything(&self) -> bool {
        !self.peaks.is_empty()
    }

    /// The peaks, hottest first.
    pub fn peaks(&self) -> Vec<Heat> {
        let mut peaks = self.peaks.clone();
        peaks.sort_by(|a, b| {
            b.peak_c
                .partial_cmp(&a.peak_c)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        peaks
    }

    /// The first sensor, if any, that has gone past the temperature it should
    /// be stopped at.
    pub fn too_hot(readings: &[Reading]) -> Option<(String, f32, f32)> {
        for reading in readings {
            let ceiling = Heat {
                label: reading.label.clone(),
                peak_c: reading.celsius,
                critical_c: reading.critical_c,
            }
            .ceiling();
            if reading.celsius >= ceiling {
                return Some((reading.label.clone(), reading.celsius, ceiling));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(label: &str, celsius: f32, critical: Option<f32>) -> Reading {
        Reading {
            label: label.to_string(),
            celsius,
            critical_c: critical,
        }
    }

    #[test]
    fn the_machines_own_critical_temperature_beats_our_guess() {
        // A laptop that says 85C is critical should be stopped well below the
        // 95C we would otherwise assume, and a workstation rated to 105C
        // should not be stopped at 95C for no reason.
        let hot_laptop = Heat {
            label: "Package".into(),
            peak_c: 0.0,
            critical_c: Some(85.0),
        };
        assert_eq!(hot_laptop.ceiling(), 85.0 - MARGIN_C);

        let workstation = Heat {
            label: "Package".into(),
            peak_c: 0.0,
            critical_c: Some(105.0),
        };
        assert!(workstation.ceiling() > DEFAULT_CEILING_C);
    }

    #[test]
    fn a_machine_that_states_no_critical_temperature_gets_the_conservative_one() {
        let unknown = Heat {
            label: "acpitz".into(),
            peak_c: 0.0,
            critical_c: None,
        };
        assert_eq!(unknown.ceiling(), DEFAULT_CEILING_C);
    }

    #[test]
    fn a_sensor_claiming_an_impossible_critical_temperature_is_not_believed() {
        // Seen in the wild: a critical of 0, which would stop every run
        // instantly, and criticals in the hundreds, which would stop none.
        for nonsense in [0.0, -40.0, 3000.0] {
            let heat = Heat {
                label: "nonsense".into(),
                peak_c: 0.0,
                critical_c: Some(nonsense),
            };
            assert_eq!(
                heat.ceiling(),
                DEFAULT_CEILING_C,
                "believed a critical of {nonsense}"
            );
        }
    }

    #[test]
    fn peaks_are_kept_per_sensor_and_only_ever_rise() {
        let mut thermometer = Thermometer::blind();
        thermometer.record(&reading("CPU", 60.0, None));
        thermometer.record(&reading("GPU", 40.0, None));
        thermometer.record(&reading("CPU", 81.0, None));
        // Cooling down must not erase the peak; the whole value of a burn-in
        // is what happened while it was running, not where it ended up.
        thermometer.record(&reading("CPU", 45.0, None));

        let peaks = thermometer.peaks();
        assert_eq!(peaks.len(), 2);
        assert_eq!(peaks[0].label, "CPU", "hottest should sort first");
        assert_eq!(peaks[0].peak_c, 81.0);
        assert_eq!(peaks[1].peak_c, 40.0);
    }

    #[test]
    fn a_critical_temperature_reported_late_is_still_picked_up() {
        let mut thermometer = Thermometer::blind();
        thermometer.record(&reading("CPU", 60.0, None));
        thermometer.record(&reading("CPU", 62.0, Some(90.0)));
        assert_eq!(thermometer.peaks()[0].critical_c, Some(90.0));
    }

    #[test]
    fn nothing_read_is_reported_as_nothing_read() {
        // Not as "it never got hot". A blind run must not look like a cool one.
        let mut thermometer = Thermometer::blind();
        assert!(thermometer.read().is_empty());
        assert!(!thermometer.saw_anything());
    }

    #[test]
    fn the_run_stops_when_a_sensor_passes_its_own_ceiling() {
        let hot = vec![
            reading("GPU", 55.0, None),
            reading("Package", 88.0, Some(90.0)),
        ];
        let (label, reached, ceiling) = Thermometer::too_hot(&hot).expect("should stop");
        assert_eq!(label, "Package");
        assert_eq!(reached, 88.0);
        assert_eq!(ceiling, 87.0);
    }

    #[test]
    fn a_warm_machine_is_not_a_reason_to_stop() {
        let warm = vec![reading("Package", 84.0, Some(100.0))];
        assert!(Thermometer::too_hot(&warm).is_none());
    }
}
