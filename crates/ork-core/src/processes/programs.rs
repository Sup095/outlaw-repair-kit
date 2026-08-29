//! Several processes that a person reads as one program.
//!
//! The list of running processes is a list of processes, and nobody thinks in
//! processes. Thirteen rows of `claude.exe` are one application to whoever is
//! looking, `chrome.exe` is one browser however many tabs it has open, and the
//! number that means anything to them is the total. This does not replace the
//! per-process list -- that list is the honest one and it is what a person
//! checks when a number looks wrong -- it groups it.
//!
//! It exists now, before anything can stop anything, because it settles a
//! question the stopping stage cannot avoid. A confirmation saying "Stop 23
//! programs?" is wrong twice: 23 is the number of processes, and the word is
//! the wrong one. Getting that sentence right afterwards would mean grouping
//! at the moment of highest consequence, from inside a dialog, which is the
//! worst place to discover that a group is not all one thing.
//!
//! Because that is the finding: **a group is usually not all one thing.** A
//! browser has processes a sweep would offer and processes it holds back, and
//! stopping the offered ones leaves the program running. Saying "this would
//! close Chrome" would be a lie in a way this tool cannot afford, so the
//! outcome of a sweep over a group is a separate answer from the group itself,
//! and it can say *part of it*.

use crate::processes::standing::{Protection, Restraint, Standing};
use crate::processes::survey::Survey;

/// What a sweep would do to a program, taken as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", tag = "how")]
pub enum Sweep {
    /// Every process belonging to it would be offered. The program would go.
    AllOfIt,
    /// Some would be offered and some would not.
    ///
    /// The program keeps running, with fewer processes than it had. That is
    /// worth saying out loud: it is the case where somebody ticks a box, sees
    /// the memory come back, and then finds the application still there --
    /// which reads as a failure unless it was said in advance.
    PartOfIt { offered: usize, remaining: usize },
    /// None of it would be offered, whether held back or never touchable.
    NoneOfIt,
}

impl Sweep {
    /// A sentence for a person, not a label.
    pub fn describe(self) -> String {
        match self {
            Sweep::AllOfIt => "all of it would be offered".to_string(),
            Sweep::PartOfIt { offered, remaining } => format!(
                "{offered} of its {} processes would be offered; the other {remaining} \
                 would be left, so the program keeps running",
                offered + remaining
            ),
            Sweep::NoneOfIt => "none of it would be offered".to_string(),
        }
    }

    /// The same answer short enough for a column.
    ///
    /// Here rather than in each front-end because there are two of them and
    /// this is four words that must not disagree. "all offered" next to
    /// eleven-of-fourteen on the other screen is the kind of difference
    /// nobody reports and everybody notices.
    pub fn briefly(self) -> String {
        match self {
            Sweep::AllOfIt => "all offered".to_string(),
            Sweep::PartOfIt { offered, remaining } => {
                format!("{offered} of {} offered", offered + remaining)
            }
            Sweep::NoneOfIt => "none offered".to_string(),
        }
    }
}

/// One program, and every process running under its name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Program {
    /// As the machine reports it, from the first process seen under this name.
    pub name: String,
    /// Every process in the group, heaviest first, so a caller can act on
    /// exactly what it showed rather than on a fresh look at a moved machine.
    pub pids: Vec<u32>,
    /// What the group holds between them. See the note in `survey.rs` about
    /// why this is never called "what would be freed": shared pages are
    /// counted against every process sharing them, and grouping makes that
    /// worse rather than better, because processes of one program share the
    /// most.
    pub memory_bytes: u64,
    /// How long the longest-running of them has been up.
    ///
    /// The longest rather than the newest: a program that has been running for
    /// three days has been running for three days, whatever it started five
    /// minutes ago.
    pub run_time_secs: u64,
    /// How many would be offered by a sweep, ticked.
    pub offered: usize,
    /// How many are held back, and the reasons given, most first.
    pub held_back: Vec<(Restraint, usize)>,
    /// How many are never touchable, and the reasons given, most first.
    pub protected: Vec<(Protection, usize)>,
}

impl Program {
    /// How many processes are running under this name.
    pub fn processes(&self) -> usize {
        self.pids.len()
    }

    /// How many are held back, whatever the reason.
    pub fn held_back_count(&self) -> usize {
        self.held_back.iter().map(|(_, count)| count).sum()
    }

    /// How many are never touchable, whatever the reason.
    pub fn protected_count(&self) -> usize {
        self.protected.iter().map(|(_, count)| count).sum()
    }

    /// What a sweep would do to it.
    pub fn sweep(&self) -> Sweep {
        let total = self.processes();
        let remaining = total - self.offered;
        if self.offered == 0 {
            Sweep::NoneOfIt
        } else if remaining == 0 {
            Sweep::AllOfIt
        } else {
            Sweep::PartOfIt {
                offered: self.offered,
                remaining,
            }
        }
    }
}

/// Count reasons, most first, in the same shape the survey already uses.
fn tally<T: Copy + PartialEq>(reasons: impl Iterator<Item = T>) -> Vec<(T, usize)> {
    let mut counts: Vec<(T, usize)> = Vec::new();
    for reason in reasons {
        match counts.iter_mut().find(|(seen, _)| *seen == reason) {
            Some((_, count)) => *count += 1,
            None => counts.push((reason, 1)),
        }
    }
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    counts
}

/// Group a survey's rows by the name the machine gives them.
///
/// Case-insensitively, because Windows reports the same executable with
/// different capitalisation depending on how it was started, and two groups
/// called `Chrome.exe` and `chrome.exe` would be a bug that only appears on
/// somebody else's machine. The name kept is the first one seen, so the screen
/// shows what the machine said rather than a lowercased version of it.
pub fn by_program(survey: &Survey) -> Vec<Program> {
    let mut programs: Vec<Program> = Vec::new();
    let mut standings: Vec<Vec<Standing>> = Vec::new();

    for row in &survey.rows {
        let key = row.name.to_ascii_lowercase();
        let at = programs
            .iter()
            .position(|program| program.name.to_ascii_lowercase() == key);
        let at = match at {
            Some(at) => at,
            None => {
                programs.push(Program {
                    name: row.name.clone(),
                    pids: Vec::new(),
                    memory_bytes: 0,
                    run_time_secs: 0,
                    offered: 0,
                    held_back: Vec::new(),
                    protected: Vec::new(),
                });
                standings.push(Vec::new());
                programs.len() - 1
            }
        };
        let program = &mut programs[at];
        program.pids.push(row.pid);
        program.memory_bytes = program.memory_bytes.saturating_add(row.memory_bytes);
        program.run_time_secs = program.run_time_secs.max(row.run_time_secs);
        if row.standing.stopped_by_default() {
            program.offered += 1;
        }
        standings[at].push(row.standing.clone());
    }

    for (program, standings) in programs.iter_mut().zip(standings) {
        program.held_back = tally(standings.iter().filter_map(|standing| match standing {
            Standing::HeldBack { because } => Some(*because),
            _ => None,
        }));
        program.protected = tally(standings.iter().filter_map(|standing| match standing {
            Standing::Protected { because } => Some(*because),
            _ => None,
        }));
    }

    // Heaviest first, matching the per-process list, because the question
    // somebody brings to either is the same one.
    programs.sort_by_key(|program| std::cmp::Reverse(program.memory_bytes));
    programs
}

impl Survey {
    /// The same rows, grouped by the program a person would say they belong to.
    pub fn by_program(&self) -> Vec<Program> {
        by_program(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PlatformKind;
    use crate::processes::in_front::InFront;
    use crate::processes::survey::Row;

    fn row(name: &str, pid: u32, memory_mb: u64, run_time_secs: u64, standing: Standing) -> Row {
        Row {
            pid,
            name: name.to_string(),
            memory_bytes: memory_mb * 1024 * 1024,
            run_time_secs,
            standing,
        }
    }

    fn survey(rows: Vec<Row>) -> Survey {
        Survey {
            rows,
            platform: PlatformKind::Windows,
            in_front: InFront::Nothing,
        }
    }

    #[test]
    fn processes_of_one_name_become_one_program() {
        let found = survey(vec![
            row("claude.exe", 1, 100, 60, Standing::Candidate),
            row("claude.exe", 2, 50, 300, Standing::Candidate),
            row("notepad.exe", 3, 10, 5, Standing::Candidate),
        ])
        .by_program();

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "claude.exe");
        assert_eq!(found[0].processes(), 3 - 1);
        assert_eq!(found[0].pids, vec![1, 2]);
        assert_eq!(found[0].memory_bytes, 150 * 1024 * 1024);
        // The longest, not the last seen.
        assert_eq!(found[0].run_time_secs, 300);
    }

    #[test]
    fn the_same_program_in_two_capitalisations_is_one_program() {
        // Windows reports the same executable differently depending on how it
        // was started, and two groups for one program is the kind of fault
        // that only ever appears on somebody else's machine.
        let found = survey(vec![
            row("Chrome.exe", 1, 100, 60, Standing::Candidate),
            row("chrome.exe", 2, 100, 60, Standing::Candidate),
        ])
        .by_program();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].processes(), 2);
        // And the name shown is what the machine said, not a lowercased one.
        assert_eq!(found[0].name, "Chrome.exe");
    }

    #[test]
    fn a_program_a_sweep_would_only_partly_stop_says_so() {
        // The case this whole module exists for. Somebody ticks the box, the
        // memory comes back, and the application is still on screen -- which
        // reads as the tool having failed, unless it was said in advance.
        let found = survey(vec![
            row("chrome.exe", 1, 100, 60, Standing::Candidate),
            row("chrome.exe", 2, 100, 60, Standing::Candidate),
            row(
                "chrome.exe",
                3,
                100,
                60,
                Standing::HeldBack {
                    because: Restraint::InFrontOfYou,
                },
            ),
        ])
        .by_program();

        assert_eq!(
            found[0].sweep(),
            Sweep::PartOfIt {
                offered: 2,
                remaining: 1
            }
        );
        assert!(found[0].sweep().describe().contains("keeps running"));
    }

    #[test]
    fn a_program_that_would_go_entirely_says_that_instead() {
        let found = survey(vec![
            row("notepad.exe", 1, 10, 60, Standing::Candidate),
            row("notepad.exe", 2, 10, 60, Standing::Candidate),
        ])
        .by_program();
        assert_eq!(found[0].sweep(), Sweep::AllOfIt);
    }

    #[test]
    fn a_program_nothing_would_touch_says_that_too() {
        let found = survey(vec![
            row(
                "csrss.exe",
                1,
                10,
                60,
                Standing::Protected {
                    because: Protection::OperatingSystem,
                },
            ),
            row(
                "csrss.exe",
                2,
                10,
                60,
                Standing::HeldBack {
                    because: Restraint::RunsAsAnotherAccount,
                },
            ),
        ])
        .by_program();

        assert_eq!(found[0].sweep(), Sweep::NoneOfIt);
        assert_eq!(found[0].protected_count(), 1);
        assert_eq!(found[0].held_back_count(), 1);
        // Held back and protected are different answers and stay apart: one
        // can be chosen deliberately and the other cannot, and a group that
        // merged them would offer something that is never offered.
        assert_eq!(found[0].protected, vec![(Protection::OperatingSystem, 1)]);
        assert_eq!(
            found[0].held_back,
            vec![(Restraint::RunsAsAnotherAccount, 1)]
        );
    }

    #[test]
    fn the_reasons_are_counted_most_first() {
        let mut rows = Vec::new();
        for pid in 1..=3 {
            rows.push(row(
                "sync.exe",
                pid,
                10,
                60,
                Standing::HeldBack {
                    because: Restraint::MayBeSyncingFiles,
                },
            ));
        }
        rows.push(row(
            "sync.exe",
            4,
            10,
            60,
            Standing::HeldBack {
                because: Restraint::JustStarted,
            },
        ));
        let found = survey(rows).by_program();
        assert_eq!(
            found[0].held_back,
            vec![
                (Restraint::MayBeSyncingFiles, 3),
                (Restraint::JustStarted, 1)
            ]
        );
    }

    #[test]
    fn programs_are_ordered_by_what_they_hold() {
        let found = survey(vec![
            row("small.exe", 1, 10, 60, Standing::Candidate),
            row("big.exe", 2, 500, 60, Standing::Candidate),
            row("middling.exe", 3, 100, 60, Standing::Candidate),
        ])
        .by_program();
        let names: Vec<&str> = found.iter().map(|program| program.name.as_str()).collect();
        assert_eq!(names, vec!["big.exe", "middling.exe", "small.exe"]);
    }

    #[test]
    fn every_process_in_the_survey_ends_up_in_exactly_one_program() {
        // The property that makes the totals on the grouped screen match the
        // totals on the per-process one. A row lost here would make the two
        // screens disagree about the same machine, and the per-process list is
        // the one people check when a number looks wrong.
        let rows = vec![
            row("a.exe", 1, 10, 60, Standing::Candidate),
            row("b.exe", 2, 20, 60, Standing::Candidate),
            row("a.exe", 3, 30, 60, Standing::Candidate),
            row(
                "c.exe",
                4,
                40,
                60,
                Standing::Protected {
                    because: Protection::Security,
                },
            ),
        ];
        let held: u64 = rows.iter().map(|row| row.memory_bytes).sum();
        let found = survey(rows).by_program();

        let mut pids: Vec<u32> = found
            .iter()
            .flat_map(|program| program.pids.iter().copied())
            .collect();
        pids.sort_unstable();
        assert_eq!(pids, vec![1, 2, 3, 4]);
        assert_eq!(
            found
                .iter()
                .map(|program| program.memory_bytes)
                .sum::<u64>(),
            held
        );
    }

    #[test]
    fn the_short_form_and_the_sentence_agree_about_the_same_program() {
        // Both are published and both are read: the column on screen and the
        // sentence behind it. Saying "all offered" over a tooltip that says
        // some would be left is worse than saying nothing.
        for sweep in [
            Sweep::AllOfIt,
            Sweep::NoneOfIt,
            Sweep::PartOfIt {
                offered: 11,
                remaining: 3,
            },
        ] {
            let short = sweep.briefly();
            let long = sweep.describe();
            assert!(!short.is_empty() && !long.is_empty());
            match sweep {
                Sweep::AllOfIt => {
                    assert!(short.starts_with("all") && long.starts_with("all of it"));
                }
                Sweep::NoneOfIt => {
                    assert!(short.starts_with("none") && long.starts_with("none of it"));
                }
                Sweep::PartOfIt { offered, remaining } => {
                    assert_eq!(short, "11 of 14 offered");
                    assert!(long.contains(&offered.to_string()));
                    assert!(long.contains(&remaining.to_string()));
                    assert!(long.contains("keeps running"));
                }
            }
        }
    }

    #[test]
    fn nothing_running_is_no_programs_rather_than_an_error() {
        assert!(survey(Vec::new()).by_program().is_empty());
    }
}
