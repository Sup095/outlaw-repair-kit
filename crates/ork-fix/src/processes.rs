//! Stopping what is running, and writing down what was stopped.
//!
//! Stage three of `docs/proposals/process-control.md`. Stages one and two
//! built the list and refused to act on it; this acts, and almost all of it is
//! about the conditions under which it will not.
//!
//! **There is no snapshot, and there cannot be one.** Every other change this
//! tool makes copies the thing first, so that "put it back" is always
//! available. A running process cannot be copied. That is not a gap to be
//! papered over with a restore button that half works -- it changes what the
//! promise is, so the promise is different here and stated plainly: the tool
//! records exactly what it stopped, and shows you, and never starts anything
//! again by itself.
//!
//! That is a decision rather than a shortcut, and the reason is worth keeping
//! next to the code. Restarting a program faithfully means recording the
//! command line it was started with, and a command line is where a password
//! passed as an argument lives. A tool that captured those in order to be
//! helpful would be writing somebody's credentials into an audit log and, from
//! there, into any bug report made afterwards. Telling somebody what was
//! stopped costs nothing and puts nothing anywhere new.
//!
//! **A list is not a permission slip.** Everything here is judged again at the
//! moment of acting, against a fresh look at the machine, because the list a
//! person pressed a button on was drawn seconds or minutes ago -- and in
//! between, a program can come to the front, a process can end, and an
//! identifier can be handed to something else entirely.

use ork_core::processes::{Standing, Stopping, Survey, stop_process};

use crate::Result;
use crate::store::FixStore;

/// One process a caller is asking to have stopped.
///
/// The name travels with the identifier deliberately. Process identifiers are
/// reused, and the gap between a list being drawn and a button being pressed
/// is exactly long enough for one to be handed to something else. Being asked
/// for both means the tool can refuse when they no longer agree, instead of
/// stopping whatever happens to be wearing that number now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Target {
    pub pid: u32,
    pub name: String,
}

/// What happened to one target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum Outcome {
    /// It was stopped.
    Stopped,
    /// It had already ended before it was asked.
    AlreadyGone,
    /// It was asked and it is still there. Nothing escalates.
    StillRunning,
    /// The operating system would not.
    Refused { because: String },
    /// The machine has changed its mind since the list was drawn: this is no
    /// longer something a sweep would offer.
    NoLongerOffered { because: String },
    /// That identifier now belongs to a different program.
    SomethingElseNow { running: String },
}

impl Outcome {
    /// Whether the machine was changed by this.
    pub fn changed_anything(&self) -> bool {
        matches!(self, Outcome::Stopped)
    }

    /// A sentence, in the tool's own words.
    pub fn describe(&self) -> String {
        match self {
            Outcome::Stopped => "stopped".to_string(),
            Outcome::AlreadyGone => "had already ended on its own".to_string(),
            Outcome::StillRunning => "was asked to stop and is still running".to_string(),
            Outcome::Refused { because } => format!("could not be stopped: {because}"),
            Outcome::NoLongerOffered { because } => {
                format!("was left alone: {because}")
            }
            Outcome::SomethingElseNow { running } => {
                format!("was not stopped: that process is now {running}")
            }
        }
    }
}

/// One target and what came of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Attempt {
    pub pid: u32,
    pub name: String,
    // Deliberately no path. It would be the one thing that makes starting
    // something again easy, and the survey this is judged from does not carry
    // one -- widening it to would put the person's home directory into every
    // `--json` survey and into anything a paired machine is shown. The name is
    // enough to find a program again, and it is not a location.
    /// What it was holding when it was last looked at. Held, not freed: see
    /// the note in `survey.rs` about why those are different numbers.
    pub memory_held_bytes: u64,
    pub outcome: Outcome,
}

/// Everything one request came to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StopReport {
    pub attempts: Vec<Attempt>,
}

impl StopReport {
    pub fn stopped(&self) -> impl Iterator<Item = &Attempt> {
        self.attempts
            .iter()
            .filter(|attempt| attempt.outcome.changed_anything())
    }

    /// How many were stopped.
    pub fn stopped_count(&self) -> usize {
        self.stopped().count()
    }

    /// Everything that was asked for and did not happen, for whatever reason.
    ///
    /// Not called "failed". Half of these are the tool declining on purpose,
    /// and a screen that filed a refusal under failures would be teaching
    /// people to distrust the part of this that works.
    pub fn did_not_stop(&self) -> impl Iterator<Item = &Attempt> {
        self.attempts
            .iter()
            .filter(|attempt| !attempt.outcome.changed_anything())
    }

    /// What the stopped processes were holding between them.
    ///
    /// Held when last seen, which is not what came back to the machine. The
    /// second number is smaller and is only knowable by measuring afterwards.
    pub fn memory_held_by_stopped(&self) -> u64 {
        self.stopped()
            .map(|attempt| attempt.memory_held_bytes)
            .sum()
    }
}

/// Stop the named processes, one at a time, judging each again first.
///
/// `pinned` is the leave-alone list, passed in rather than read here so that
/// this can be tested and so that the judgement is made against exactly the
/// settings the caller was showing.
///
/// Nothing is stopped concurrently. A sweep that stopped ten things at once
/// would make the audit log a race, and there would be no moment at which the
/// machine could be looked at between two changes -- which is the whole
/// meaning of "one change at a time".
pub fn stop_these(targets: &[Target], pinned: &[String], store: &FixStore) -> Result<StopReport> {
    let mut attempts = Vec::new();

    for target in targets {
        // A fresh look for every single one, not one look for the batch. The
        // first thing stopped changes the machine, and what the second one is
        // now part of may not be what it was part of a moment ago -- the
        // launcher whose game just ended, the browser that is now in front.
        let survey = Survey::of_this_machine(pinned)?;
        let attempt = judge_and_stop(target, &survey);

        // Written down whatever happened, including the refusals. An audit
        // log that recorded only the changes would answer "what did this tool
        // do to my machine" and not "what did I ask it to do", and the second
        // question is the one somebody asks when something is missing.
        store.audit(
            if attempt.outcome.changed_anything() {
                "process-stopped"
            } else {
                "process-left-alone"
            },
            &format!(
                "{} ({}) {}",
                attempt.name,
                attempt.pid,
                attempt.outcome.describe()
            ),
            serde_json::to_string(&attempt).ok().as_deref(),
        )?;

        attempts.push(attempt);
    }

    Ok(StopReport { attempts })
}

/// Judge one target against a fresh survey, and stop it if it still qualifies.
fn judge_and_stop(target: &Target, survey: &Survey) -> Attempt {
    match judge(target, survey) {
        Err(refused) => refused,
        Ok(allowed) => {
            let outcome = match stop_process(allowed.pid) {
                Stopping::Stopped => Outcome::Stopped,
                Stopping::AlreadyGone => Outcome::AlreadyGone,
                Stopping::StillRunning => Outcome::StillRunning,
                Stopping::Refused { because } => Outcome::Refused { because },
            };
            Attempt { outcome, ..allowed }
        }
    }
}

/// Whether this target may still be stopped, and what to say if not.
///
/// Deliberately separate from the stopping, and it is not only tidiness: this
/// is the half worth testing exhaustively, and a test of the combined function
/// would have to reach the real kill to find out what it decided. An early
/// draft of this file did exactly that -- a test that meant to check a name
/// comparison ran against a live process identifier on the machine running the
/// tests. It passed because that identifier happened not to exist.
///
/// `Ok` carries the attempt as it stands before acting, so the caller fills in
/// only the outcome. `Err` is the finished answer.
fn judge(target: &Target, survey: &Survey) -> std::result::Result<Attempt, Attempt> {
    let row = survey.rows.iter().find(|row| row.pid == target.pid);

    let Some(row) = row else {
        return Err(Attempt {
            pid: target.pid,
            name: target.name.clone(),
            memory_held_bytes: 0,
            outcome: Outcome::AlreadyGone,
        });
    };

    let seen = Attempt {
        pid: row.pid,
        name: row.name.clone(),
        memory_held_bytes: row.memory_bytes,
        outcome: Outcome::Stopped,
    };

    // The identifier is right and the program is not. Compared the way every
    // other name comparison in this tool is made, because a program reported
    // as `Steam.exe` once and `steam.exe` the next time is the same program
    // and refusing over it would be a fault of its own.
    if !row.name.eq_ignore_ascii_case(&target.name) {
        return Err(Attempt {
            outcome: Outcome::SomethingElseNow {
                running: row.name.clone(),
            },
            ..seen
        });
    }

    // Judged again, now. This is the rail that matters: the list was drawn
    // before the person pressed anything, and a program that has come to the
    // front since then is a program they are looking at.
    if !row.standing.stopped_by_default() {
        return Err(Attempt {
            outcome: Outcome::NoLongerOffered {
                because: because(&row.standing),
            },
            ..seen
        });
    }

    Ok(seen)
}

/// The reason a standing gives, in the words the rest of the tool uses.
fn because(standing: &Standing) -> String {
    match standing {
        Standing::Protected { because } => because.describe().to_string(),
        Standing::HeldBack { because } => because.describe().to_string(),
        // Unreachable from the caller above, and written out rather than
        // panicked on: a sweep that fell over because something became
        // stoppable would be a worse fault than the one being guarded.
        Standing::Candidate => "it is offered".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ork_core::platform::PlatformKind;
    use ork_core::processes::{InFront, Restraint, Row};

    fn survey_of(rows: Vec<Row>) -> Survey {
        Survey {
            rows,
            platform: PlatformKind::Windows,
            in_front: InFront::Nothing,
        }
    }

    fn row(pid: u32, name: &str, standing: Standing) -> Row {
        Row {
            pid,
            name: name.to_string(),
            memory_bytes: 10 * 1024 * 1024,
            run_time_secs: 3_600,
            standing,
        }
    }

    #[test]
    fn a_process_that_is_no_longer_offered_is_not_stopped() {
        // The rail this whole file rests on. The list was drawn before the
        // button was pressed, and in between the program came to the front of
        // the screen -- so the machine is asked again rather than the list
        // being believed.
        let survey = survey_of(vec![row(
            7,
            "SomeGame.exe",
            Standing::HeldBack {
                because: Restraint::InFrontOfYou,
            },
        )]);
        let refused = judge(
            &Target {
                pid: 7,
                name: "SomeGame.exe".to_string(),
            },
            &survey,
        )
        .expect_err("a program in front of you must not be stoppable");
        assert_eq!(
            refused.outcome,
            Outcome::NoLongerOffered {
                because: "in front of you right now".to_string()
            }
        );
        assert!(!refused.outcome.changed_anything());
    }

    #[test]
    fn a_reused_identifier_stops_nothing() {
        // Process identifiers are handed out again, and the gap between a list
        // and a button is long enough for it to happen. Without this the tool
        // would stop whatever was wearing that number by then, and the audit
        // log would say it stopped the program the person asked for.
        let survey = survey_of(vec![row(7, "SomethingElse.exe", Standing::Candidate)]);
        let refused = judge(
            &Target {
                pid: 7,
                name: "SomeGame.exe".to_string(),
            },
            &survey,
        )
        .expect_err("a reused identifier must not be stoppable");
        assert_eq!(
            refused.outcome,
            Outcome::SomethingElseNow {
                running: "SomethingElse.exe".to_string()
            }
        );
    }

    #[test]
    fn the_same_program_in_a_different_case_is_still_the_same_program() {
        // The other direction, and it would be a fault of its own. Windows
        // reports the same executable differently depending on how it was
        // started, and refusing over a capital letter would make the button
        // fail for a reason nobody could see.
        let survey = survey_of(vec![row(7, "Steam.exe", Standing::Candidate)]);
        let allowed = judge(
            &Target {
                pid: 7,
                name: "steam.exe".to_string(),
            },
            &survey,
        )
        .expect("a capital letter must not stop the button working");
        // Nothing is stopped by asking this question, which is the point of
        // asking it separately.
        assert_eq!(allowed.pid, 7);
        assert_eq!(allowed.name, "Steam.exe");
    }

    #[test]
    fn a_process_that_ended_on_its_own_is_not_an_error() {
        // Between a list being drawn and a button being pressed, programs
        // finish. Reporting that as a failure would have somebody hunting for
        // a fault that is the ordinary behaviour of a computer.
        let survey = survey_of(Vec::new());
        // Deliberately a real-looking identifier and nothing behind it, which
        // is the ordinary case: the survey is a moment old and the program
        // finished in that moment.

        let refused = judge(
            &Target {
                pid: 7,
                name: "SomeUpdater.exe".to_string(),
            },
            &survey,
        )
        .expect_err("a process that is gone cannot be stopped");
        assert_eq!(refused.outcome, Outcome::AlreadyGone);
        assert!(!refused.outcome.changed_anything());
    }

    #[test]
    fn nothing_that_is_never_touched_can_be_reached_through_this() {
        // Belt and braces. A protected process should never appear in a list
        // anybody could press a button on, and if one ever did, this is where
        // it stops.
        use ork_core::processes::Protection;
        let survey = survey_of(vec![row(
            4,
            "csrss.exe",
            Standing::Protected {
                because: Protection::OperatingSystem,
            },
        )]);
        let refused = judge(
            &Target {
                pid: 4,
                name: "csrss.exe".to_string(),
            },
            &survey,
        )
        .expect_err("nothing protected may be reached through this");
        assert_eq!(
            refused.outcome,
            Outcome::NoLongerOffered {
                because: "part of the operating system".to_string()
            }
        );
    }

    #[test]
    fn every_outcome_reads_as_a_sentence_and_none_of_them_are_the_same() {
        // These are printed at somebody about their own machine. A blank one,
        // or two that say the same thing, is a screen that cannot be acted on.
        let all = [
            Outcome::Stopped,
            Outcome::AlreadyGone,
            Outcome::StillRunning,
            Outcome::Refused {
                because: "denied".to_string(),
            },
            Outcome::NoLongerOffered {
                because: "in front of you right now".to_string(),
            },
            Outcome::SomethingElseNow {
                running: "other.exe".to_string(),
            },
        ];
        let mut said = Vec::new();
        for outcome in &all {
            let sentence = outcome.describe();
            assert!(!sentence.is_empty(), "{outcome:?} has no words");
            assert!(
                !said.contains(&sentence),
                "two outcomes both say {sentence:?}"
            );
            said.push(sentence);
        }
        // And exactly one of them means the machine changed.
        assert_eq!(
            all.iter()
                .filter(|outcome| outcome.changed_anything())
                .count(),
            1
        );
    }

    #[test]
    fn a_report_counts_what_it_changed_and_not_what_it_declined() {
        let report = StopReport {
            attempts: vec![
                Attempt {
                    pid: 1,
                    name: "a.exe".to_string(),
                    memory_held_bytes: 100,
                    outcome: Outcome::Stopped,
                },
                Attempt {
                    pid: 2,
                    name: "b.exe".to_string(),
                    memory_held_bytes: 900,
                    outcome: Outcome::NoLongerOffered {
                        because: "in front of you right now".to_string(),
                    },
                },
                Attempt {
                    pid: 3,
                    name: "c.exe".to_string(),
                    memory_held_bytes: 50,
                    outcome: Outcome::AlreadyGone,
                },
            ],
        };
        assert_eq!(report.stopped_count(), 1);
        assert_eq!(report.did_not_stop().count(), 2);
        // Only what was actually stopped counts towards the total, or the
        // number would be a claim about memory the tool never touched.
        assert_eq!(report.memory_held_by_stopped(), 100);
    }
}
