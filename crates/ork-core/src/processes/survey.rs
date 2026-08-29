//! Looking at everything running, and saying what would happen to each.
//!
//! Still stops nothing. This is stage two of the plan in
//! `docs/proposals/process-control.md`: the list, so that it can be looked at
//! on real machines before anything is able to act on it. A list that has been
//! read by people is the only thing that makes the button afterwards
//! defensible.
//!
//! Two things here are deliberately awkward, and both are honesty rather than
//! caution.
//!
//! **Memory is described as held, never as freed.** A process's working set is
//! not the same as memory that comes back to the machine when it stops: shared
//! pages are counted against every process sharing them, and some of what a
//! process holds is already on disk. Adding those numbers up gives a figure
//! that is always too big. So this reports what is *held*, which is a
//! measurement, and leaves "freed" for the stage that measures it afterwards.
//!
//! **What is left alone is counted and named, not hidden.** A tool's list of
//! what it considers untouchable is worthless if nobody can read it.

use crate::platform::{PlatformKind, ProcessInfo};
use crate::processes::in_front;
use crate::processes::in_front::InFront;
use crate::processes::standing::{
    Circumstances, Protection, Restraint, Standing, classify, family_of, lineage_of,
};

/// One running process, and what would happen to it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Row {
    pub pid: u32,
    pub name: String,
    /// What the process holds right now. See the note above about why this is
    /// never called "what would be freed".
    pub memory_bytes: u64,
    pub run_time_secs: u64,
    pub standing: Standing,
}

/// Everything running, judged.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Survey {
    pub rows: Vec<Row>,
    /// The platform the judgement was made for, which is not necessarily the
    /// one it was made on -- a scan can be read from a paired machine.
    pub platform: PlatformKind,
    /// What had the window in front of the person, when this was taken.
    ///
    /// Carried on the survey rather than left on the circumstances that
    /// produced it, because the one thing anything showing this list must be
    /// able to do is say when the rail could not run. A survey that cannot be
    /// asked that question reads as though every rail was applied.
    #[serde(default)]
    pub in_front: InFront,
}

impl Survey {
    /// Judge a list of processes.
    ///
    /// Takes the processes rather than fetching them so that this can be run
    /// against a machine that is not this one, and tested without one.
    pub fn of(processes: &[ProcessInfo], platform: PlatformKind, about: &Circumstances) -> Survey {
        let mut rows: Vec<Row> = processes
            .iter()
            .map(|process| Row {
                pid: process.pid,
                name: process.name.clone(),
                memory_bytes: process.memory_bytes,
                run_time_secs: process.run_time_secs,
                standing: classify(process, platform, about),
            })
            .collect();

        // Heaviest first, because the only question anybody brings to this
        // list is what is holding the machine's memory.
        rows.sort_by_key(|row| std::cmp::Reverse(row.memory_bytes));
        Survey {
            rows,
            platform,
            in_front: about.in_front.clone(),
        }
    }

    /// Judge what is running on this machine, right now.
    pub fn of_this_machine(pinned: &[String]) -> anyhow::Result<Survey> {
        let platform = crate::platform::detect()?;
        let processes = platform.processes()?;
        let about = Circumstances::here(&processes, pinned);
        Ok(Survey::of(&processes, platform.kind(), &about))
    }

    pub fn candidates(&self) -> impl Iterator<Item = &Row> {
        self.rows
            .iter()
            .filter(|row| row.standing.stopped_by_default())
    }

    pub fn held_back(&self) -> impl Iterator<Item = &Row> {
        self.rows
            .iter()
            .filter(|row| matches!(row.standing, Standing::HeldBack { .. }))
    }

    pub fn protected(&self) -> impl Iterator<Item = &Row> {
        self.rows
            .iter()
            .filter(|row| !row.standing.can_ever_be_stopped())
    }

    /// What the candidates are holding between them.
    ///
    /// Named for what it measures. This is not what stopping them would free,
    /// it is always more than that, and the difference is the whole reason
    /// this tool exists rather than another one that says 6 GB and delivers
    /// 900 MB.
    pub fn memory_held_by_candidates(&self) -> u64 {
        self.candidates().map(|row| row.memory_bytes).sum()
    }

    /// How many are protected, grouped by why, most first.
    pub fn why_protected(&self) -> Vec<(Protection, usize)> {
        let mut counts: Vec<(Protection, usize)> = Vec::new();
        for row in self.protected() {
            let Standing::Protected { because } = row.standing else {
                continue;
            };
            match counts.iter_mut().find(|(reason, _)| *reason == because) {
                Some((_, count)) => *count += 1,
                None => counts.push((because, 1)),
            }
        }
        counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        counts
    }

    /// The whole survey as machine-readable output.
    ///
    /// One shape, built here, because both front-ends publish it and they used
    /// to build it separately -- the terminal's `--json` and the window's
    /// `process_survey` were two hand-written copies of the same object, and
    /// they had already drifted by three keys. Something reading either is
    /// reading a contract, and a contract with two authors is two contracts.
    ///
    /// Presentation still belongs to the front-ends. This is not presentation:
    /// it is the answer, and both of them answer the same question.
    pub fn as_report(&self) -> serde_json::Value {
        let programs = self.by_program();
        serde_json::json!({
            "platform": self.platform.as_str(),
            "running": self.rows.len(),
            "protected": self.protected().count(),
            "held_back": self.held_back().count(),
            "candidates": self.candidates().count(),
            // Named as it is measured, in the machine-readable output as much
            // as on screen. Nothing reading this may honestly print it as
            // "will free".
            "memory_held_by_candidates": self.memory_held_by_candidates(),
            "why_protected": self.why_protected().iter().map(|(reason, count)| {
                serde_json::json!({ "reason": reason.describe(), "count": count })
            }).collect::<Vec<_>>(),
            "why_held_back": self.why_held_back().iter().map(|(reason, count)| {
                serde_json::json!({ "reason": reason.describe(), "count": count })
            }).collect::<Vec<_>>(),
            // Null when the rule ran. Anything reading this must be able to
            // tell "nothing was in front of you" from "we could not look",
            // because only one of those is a complete list.
            "in_front_unchecked": self.in_front.unanswered(),
            // The same rows grouped the way a person reads them. Both are
            // published because they answer different questions, and the
            // per-process list is the one to check when a number looks wrong.
            "programs": programs.iter().map(|program| {
                serde_json::json!({
                    "name": program.name,
                    "pids": program.pids,
                    "processes": program.processes(),
                    "memory_held": program.memory_bytes,
                    "run_time_secs": program.run_time_secs,
                    "offered": program.offered,
                    "held_back": program.held_back_count(),
                    "protected": program.protected_count(),
                    "pinned": program.pinned(),
                    "sweep": program.sweep(),
                    "sweep_says": program.sweep().describe(),
                    "sweep_briefly": program.sweep().briefly(),
                })
            }).collect::<Vec<_>>(),
            "rows": self.rows,
        })
    }

    /// How many are held back, grouped by why, most first.
    pub fn why_held_back(&self) -> Vec<(Restraint, usize)> {
        let mut counts: Vec<(Restraint, usize)> = Vec::new();
        for row in self.held_back() {
            let Standing::HeldBack { because } = row.standing else {
                continue;
            };
            match counts.iter_mut().find(|(reason, _)| *reason == because) {
                Some((_, count)) => *count += 1,
                None => counts.push((because, 1)),
            }
        }
        counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        counts
    }
}

impl Circumstances {
    /// What can be established about this machine, right now.
    ///
    /// Deliberately modest. Where something cannot be established it is left
    /// unknown rather than guessed, and the classifier treats unknown as the
    /// careful answer -- so a gap here costs a few megabytes that could have
    /// been freed, never somebody's audio or their unsaved work.
    pub fn here(processes: &[ProcessInfo], pinned: &[String]) -> Circumstances {
        let own_lineage = lineage_of(std::process::id(), processes);
        let own_family = family_of(&own_lineage, processes);
        let in_front = in_front::ask();
        // Only widen an answer we actually got. `Unknown` and `Nothing` both
        // leave these empty, which holds nothing back -- correct, and the
        // reason the answer itself is kept so a caller can say so.
        let in_front_lineage = match in_front.pid() {
            Some(pid) => lineage_of(pid, processes),
            None => Vec::new(),
        };
        let in_front_family = family_of(&in_front_lineage, processes);
        Circumstances {
            // Trimmed as well as lower-cased, and both for the same reason:
            // this is a hand-edited list, and `ProcessConfig::is_pinned`
            // answers the same question for the window. Two answers to "is
            // this pinned" would be worse than none -- the screen would show
            // a program as left alone while the classifier offered it.
            pinned: pinned
                .iter()
                .map(|name| name.trim().to_ascii_lowercase())
                .collect(),
            in_front,
            in_front_lineage,
            in_front_family,
            own_lineage,
            own_family,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::ProcessState;

    fn process(name: &str, pid: u32, memory_mb: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent_pid: None,
            name: name.to_string(),
            // A path, because a real process of yours has one and something
            // with no path is now held back for not being restartable. A
            // fixture without one would make every candidate test here a test
            // of that rule instead of the rule it was written for.
            executable: Some(format!(r"C:\Program Files\Somewhere\{name}")),
            memory_bytes: memory_mb * 1024 * 1024,
            cpu_percent: 0.0,
            run_time_secs: 60 * 60,
            runs_as_you: Some(true),
            state: ProcessState::Running,
        }
    }

    fn nothing_special() -> Circumstances {
        Circumstances {
            own_lineage: vec![999_999],
            ..Default::default()
        }
    }

    fn survey(processes: &[ProcessInfo]) -> Survey {
        Survey::of(processes, PlatformKind::Windows, &nothing_special())
    }

    #[test]
    fn everything_running_appears_exactly_once() {
        // The list is the whole product at this stage. A process that is
        // quietly dropped is one nobody can decide about, and the drop is
        // invisible -- there is nothing on screen where it should have been.
        let processes = [
            process("MsMpEng.exe", 1, 200),
            process("SomeUpdater.exe", 2, 50),
            process("firefox.exe", 3, 900),
        ];
        let survey = survey(&processes);
        assert_eq!(survey.rows.len(), processes.len());

        let counted =
            survey.protected().count() + survey.held_back().count() + survey.candidates().count();
        assert_eq!(
            counted,
            processes.len(),
            "every process must fall into exactly one of the three groups"
        );
    }

    #[test]
    fn the_heaviest_is_first() {
        let survey = survey(&[
            process("small.exe", 1, 10),
            process("huge.exe", 2, 4000),
            process("middling.exe", 3, 300),
        ]);
        let order: Vec<&str> = survey.rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(order, ["huge.exe", "middling.exe", "small.exe"]);
    }

    #[test]
    fn memory_is_counted_only_for_what_would_actually_be_stopped() {
        // The number on screen has to describe the rows on screen. Counting
        // the protected ones in would inflate it by most of the machine.
        let survey = survey(&[
            process("MsMpEng.exe", 1, 500),    // protected: security
            process("firefox.exe", 2, 900),    // held back: unsaved work
            process("SomeUpdater.exe", 3, 40), // candidate
        ]);
        assert_eq!(
            survey.memory_held_by_candidates(),
            40 * 1024 * 1024,
            "only the candidate's memory belongs in the total"
        );
    }

    #[test]
    fn nothing_is_counted_when_nothing_would_be_stopped() {
        let survey = survey(&[process("MsMpEng.exe", 1, 500)]);
        assert_eq!(survey.memory_held_by_candidates(), 0);
        assert_eq!(survey.candidates().count(), 0);
    }

    #[test]
    fn what_is_left_alone_is_grouped_by_the_reason_it_was_left_alone() {
        // A count with no reasons attached is a tool asking to be trusted. The
        // reasons are the argument.
        let survey = survey(&[
            process("MsMpEng.exe", 1, 100),
            process("Windows Defender.exe", 2, 100),
            process("nvcontainer.exe", 3, 100),
            process("SomeUpdater.exe", 4, 10),
        ]);
        let why = survey.why_protected();
        assert!(
            !why.is_empty(),
            "nothing was protected on a list with security software in it"
        );
        let total: usize = why.iter().map(|(_, count)| count).sum();
        assert_eq!(total, survey.protected().count());

        // Most common first, so the biggest group is the one somebody reads.
        for pair in why.windows(2) {
            assert!(pair[0].1 >= pair[1].1, "reasons are not in order: {why:?}");
        }
    }

    #[test]
    fn the_reasons_for_holding_something_back_are_counted_the_same_way() {
        let survey = survey(&[
            process("firefox.exe", 1, 900),
            process("chrome.exe", 2, 800),
            process("SomeUpdater.exe", 3, 10),
        ]);
        let why = survey.why_held_back();
        let total: usize = why.iter().map(|(_, count)| count).sum();
        assert_eq!(total, survey.held_back().count());
    }

    #[test]
    fn an_empty_machine_is_an_empty_list_and_not_an_error() {
        let survey = survey(&[]);
        assert!(survey.rows.is_empty());
        assert_eq!(survey.memory_held_by_candidates(), 0);
        assert!(survey.why_protected().is_empty());
    }

    #[test]
    fn pinning_something_is_matched_however_it_was_typed() {
        // Somebody typing a program's name into a settings box will not match
        // the case the operating system reports, and being ignored silently
        // is the worst possible outcome for a control whose entire job is
        // "leave this one alone".
        let processes = [process("SomeUpdater.exe", 1, 10)];
        let about = Circumstances::here(&processes, &["SOMEUPDATER.EXE".to_string()]);
        let survey = Survey::of(&processes, PlatformKind::Windows, &about);
        assert!(
            !survey.rows[0].standing.stopped_by_default(),
            "a pinned program was offered anyway: {:?}",
            survey.rows[0].standing
        );
    }

    #[test]
    fn the_window_and_the_classifier_agree_about_what_is_pinned() {
        // Two places answer "is this pinned": this classifier, and
        // `ProcessConfig::is_pinned`, which is what a screen asks to draw the
        // control. If they disagreed, the window would show a program as left
        // alone while the list below it offered the same program -- and
        // whichever the person believed, one of them would be lying.
        //
        // Stray spaces are the case that separated them: this is a
        // hand-edited file and " steam.exe " is a thing somebody types.
        let processes = [process("SomeUpdater.exe", 1, 10)];
        for typed in [
            "SomeUpdater.exe",
            "someupdater.exe",
            "  SomeUpdater.exe  ",
            "SOMEUPDATER.EXE",
        ] {
            let settings = crate::config::ProcessConfig {
                pinned: vec![typed.to_string()],
            };
            let about = Circumstances::here(&processes, &settings.pinned);
            let survey = Survey::of(&processes, PlatformKind::Windows, &about);
            assert_eq!(
                settings.is_pinned("SomeUpdater.exe"),
                !survey.rows[0].standing.stopped_by_default(),
                "`{typed}`: the settings and the classifier disagree"
            );
            assert!(
                settings.is_pinned("SomeUpdater.exe"),
                "`{typed}` should read as pinned"
            );
        }
    }

    #[test]
    fn this_tool_is_never_in_the_list_of_what_would_be_stopped() {
        // Read from the real machine, because the point is that the gathering
        // half and the judging half agree about which process is us.
        let survey = Survey::of_this_machine(&[]).expect("this machine has processes");
        let me = std::process::id();
        assert!(
            !survey.candidates().any(|row| row.pid == me),
            "the tool offered to stop itself"
        );
    }

    #[test]
    fn the_report_answers_everything_either_front_end_asks_for() {
        // The keys are a contract. Something is reading this that neither the
        // window nor the terminal knows about, and a key that quietly went
        // away is a script that quietly started reading `null`.
        let survey = Survey::of_this_machine(&[]).expect("this machine has processes");
        let report = survey.as_report();
        for key in [
            "platform",
            "running",
            "protected",
            "held_back",
            "candidates",
            "memory_held_by_candidates",
            "why_protected",
            "why_held_back",
            "in_front_unchecked",
            "programs",
            "rows",
        ] {
            assert!(
                report.get(key).is_some(),
                "the report no longer has `{key}`, which something is reading"
            );
        }
    }

    #[test]
    fn the_report_never_calls_held_memory_freed() {
        // The one thing this tool must not say. Checked on the text of the
        // whole object rather than on one field, because the failure would
        // arrive as a helpfully-named new key rather than as a changed one.
        let survey = Survey::of_this_machine(&[]).expect("this machine has processes");
        let written = survey.as_report().to_string();
        for forbidden in ["will_free", "would_free", "freed", "memory_freed"] {
            assert!(
                !written.contains(forbidden),
                "the report contains `{forbidden}`. Adding up working sets always \
                 overstates what stopping things returns to the machine, so the \
                 word is `held` until something has measured it afterwards."
            );
        }
    }

    #[test]
    fn the_grouped_and_ungrouped_halves_describe_the_same_machine() {
        // Both are published, and somebody comparing them is the most likely
        // reader of either. If the totals disagreed, the per-process list --
        // the one people check when a number looks wrong -- would be the thing
        // casting doubt on the tool rather than confirming it.
        let survey = Survey::of_this_machine(&[]).expect("this machine has processes");
        let programs = survey.by_program();
        assert!(!programs.is_empty(), "nothing was running at all");

        let in_groups: usize = programs.iter().map(|program| program.processes()).sum();
        assert_eq!(
            in_groups,
            survey.rows.len(),
            "a process was lost in grouping"
        );

        let offered: usize = programs.iter().map(|program| program.offered).sum();
        assert_eq!(
            offered,
            survey.candidates().count(),
            "the grouped view and the list disagree about what would be offered"
        );

        let held: u64 = programs.iter().map(|program| program.memory_bytes).sum();
        assert_eq!(
            held,
            survey.rows.iter().map(|row| row.memory_bytes).sum::<u64>(),
            "the grouped totals do not add up to the list's"
        );
    }
}
