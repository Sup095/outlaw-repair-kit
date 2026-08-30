//! What each command is, written down once.
//!
//! Today there is one front-end and its command list is declared by `clap`
//! attributes in `ork-cli`. That works exactly as long as there is one
//! front-end. The moment a second one exists -- CritterScript, which is
//! [proposed and decided in outline](../../../docs/proposals/critterscript.md)
//! -- a name, a summary and an example either live somewhere both can read or
//! live twice, and two copies of a command list drift silently: the second one
//! is right about a command the first has renamed, and nothing anywhere fails.
//!
//! So they live here, in the crate both front-ends already depend on, and
//! `clap` is given its summaries from this table rather than the other way
//! round. The one thing still tied to `clap` is a single test that this table
//! and its command list are the same set -- which is the check that has to keep
//! passing while the swap happens, and the only one that gets deleted after.
//!
//! # What is not here
//!
//! The long explanation of a command. That belongs in `docs/commands.md`,
//! which is compiled into the binary, shown by the window, and read by the
//! terminal -- already the one place both front-ends get it from. Putting a
//! second paragraph here would recreate the problem this file exists to
//! prevent, one level down.
//!
//! A line and an example is what a command *list* needs. Everything longer is
//! the manual's, and there is a test below that every command in this table
//! has a section in it.

/// Where a command belongs in a list of them.
///
/// Ordering, not taxonomy. Somebody scanning a reference wants the looking
/// commands together and the changing ones together, and wants to be able to
/// tell which is which without reading each line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    /// Finding out what is wrong.
    Looking,
    /// Doing something about it.
    Acting,
    /// The tool's own settings, credentials, and paired machines.
    Setup,
    /// What the tool has done, and what it is.
    Records,
}

impl Group {
    /// The heading this group would be printed under.
    pub fn heading(self) -> &'static str {
        match self {
            Group::Looking => "Looking",
            Group::Acting => "Acting",
            Group::Setup => "Setup",
            Group::Records => "Records",
        }
    }

    /// In the order a list of them should be printed.
    pub fn all() -> [Group; 4] {
        [Group::Looking, Group::Acting, Group::Setup, Group::Records]
    }
}

/// What running a command can change.
///
/// Declared per command rather than known by whoever is calling it, because
/// the alternative is that every caller decides for itself and one of them
/// eventually decides wrongly. This is the same shape as CritterScript's
/// `guestSafe`, and for the same reason the reference implementation gives for
/// checking that in one place: two copies of the check is how the second form
/// of invocation ends up without it.
///
/// It is a statement about the command, not about a particular run of it.
/// `processes` is [`Changes::OnlyWhenAsked`] whether or not `--stop` was
/// given, because what this answers is "could this change my machine", and for
/// that question the honest answer does not depend on the arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Changes {
    /// Reads the machine and writes nothing to it.
    ///
    /// It may still write the tool's own records -- a scan is remembered, and
    /// an audit entry is written for it. That is not a change to the machine
    /// and calling it one would make the word useless.
    Nothing,
    /// Writes the tool's own settings or credentials, never the machine.
    OwnSettings,
    /// Works the hardware deliberately, and changes nothing.
    ///
    /// Its own category because "changes nothing" is true of it and is not the
    /// whole truth: it heats the machine and makes it slow to use while it
    /// runs. A list that filed it with `host` would be right and useless.
    WorksTheMachine,
    /// Can change the machine, and only when explicitly asked to.
    OnlyWhenAsked,
}

impl Changes {
    /// Said the way it would be said to a person.
    pub fn describe(self) -> &'static str {
        match self {
            Changes::Nothing => "changes nothing",
            Changes::OwnSettings => "changes this tool's own settings",
            Changes::WorksTheMachine => "works the machine hard, and changes nothing",
            Changes::OnlyWhenAsked => "can change the machine, when asked to",
        }
    }

    /// Whether this command could leave the machine different.
    pub fn could_change_the_machine(self) -> bool {
        matches!(self, Changes::OnlyWhenAsked)
    }
}

/// One command, as a list of them would show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct CommandDoc {
    /// As it is typed.
    pub name: &'static str,
    /// One line. What a reference prints beside the name, and what the
    /// terminal shows in `outlaw --help`.
    pub help: &'static str,
    /// A canonical example, complete enough to run.
    ///
    /// Checked against the real parser in `ork-cli`, so an example here that
    /// the program would reject fails the build rather than somebody's evening.
    pub usage: &'static str,
    pub group: Group,
    pub changes: Changes,
}

/// Every command this tool has.
///
/// The order within a group is the order it is printed in, so the commands
/// somebody reaches for first come first rather than alphabetically.
pub const ALL: &[CommandDoc] = &[
    CommandDoc {
        name: "scan",
        help: "Run a scan and report what is wrong.",
        usage: "outlaw scan --tier full",
        group: Group::Looking,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "watch",
        help: "Keep looking, and say something only when something changes.",
        usage: "outlaw watch --every 15",
        group: Group::Looking,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "watching",
        help: "Show what the watcher remembers, without watching.",
        usage: "outlaw watching",
        group: Group::Looking,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "processes",
        help: "Show what is running, and what a sweep would do to each.",
        usage: "outlaw processes --all",
        group: Group::Looking,
        changes: Changes::OnlyWhenAsked,
    },
    CommandDoc {
        name: "host",
        help: "Show what this tool detected about the machine it is running on.",
        usage: "outlaw host",
        group: Group::Looking,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "probes",
        help: "List the checks this build knows how to run.",
        usage: "outlaw probes",
        group: Group::Looking,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "queue",
        help: "Show problems waiting to be worked through.",
        usage: "outlaw queue",
        group: Group::Looking,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "fix",
        help: "Work through the triage queue.",
        usage: "outlaw fix --apply",
        group: Group::Acting,
        changes: Changes::OnlyWhenAsked,
    },
    CommandDoc {
        name: "stress",
        help: "Work the machine hard on purpose, and see whether it gets anything wrong.",
        usage: "outlaw stress --minutes 10",
        group: Group::Acting,
        changes: Changes::WorksTheMachine,
    },
    CommandDoc {
        name: "boot",
        help: "Run the start-up screen on its own: self-test and update check.",
        usage: "outlaw boot",
        group: Group::Acting,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "config",
        help: "Show where settings live and what they currently say.",
        usage: "outlaw config",
        group: Group::Setup,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "set-key",
        help: "Store a credential in the system credential store.",
        usage: "outlaw set-key cloud",
        group: Group::Setup,
        changes: Changes::OwnSettings,
    },
    CommandDoc {
        name: "models",
        help: "Show which model would be used, and why.",
        usage: "outlaw models",
        group: Group::Setup,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "link",
        help: "Lend a model to another machine, or borrow one.",
        usage: "outlaw link",
        group: Group::Setup,
        changes: Changes::OwnSettings,
    },
    CommandDoc {
        name: "audit",
        help: "Show everything the tool has checked, found, attempted, and changed.",
        usage: "outlaw audit --limit 40",
        group: Group::Records,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "report",
        help: "Turn a crash or an error into a bug report you can post.",
        usage: "outlaw report",
        group: Group::Records,
        changes: Changes::Nothing,
    },
    CommandDoc {
        name: "docs",
        help: "Read the manual, which is carried inside this program.",
        usage: "outlaw docs commands",
        group: Group::Records,
        changes: Changes::Nothing,
    },
];

/// One command by name.
pub fn find(name: &str) -> Option<&'static CommandDoc> {
    ALL.iter().find(|command| command.name == name)
}

/// One command's summary, or a panic naming the one that is missing.
///
/// Loud on purpose. This is what `clap` is given for each of its commands, so
/// a name that is not in the table above stops the program on any run at all,
/// including the first test that constructs the command line. A summary that
/// quietly came back empty would instead ship a blank line in `--help`.
pub fn help(name: &'static str) -> &'static str {
    match find(name) {
        Some(command) => command.help,
        None => panic!(
            "`{name}` is not in ork_core::commands::ALL, so there is nothing to \
             say about it in a list of commands. Add it there rather than here."
        ),
    }
}

/// Every command in one group, in the order they should be printed.
pub fn in_group(group: Group) -> impl Iterator<Item = &'static CommandDoc> {
    ALL.iter().filter(move |command| command.group == group)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manual, which is the long form of everything summarised here.
    const COMMANDS_PAGE: &str = include_str!("../../../docs/commands.md");

    #[test]
    fn there_is_a_table_to_check() {
        // Everything below is a filter over ALL. If it were ever empty they
        // would all pass by having nothing to disagree with.
        assert!(
            ALL.len() >= 17,
            "only {} commands are registered, which is fewer than this tool has",
            ALL.len()
        );
    }

    #[test]
    fn no_command_is_registered_twice() {
        let mut seen: Vec<&str> = ALL.iter().map(|command| command.name).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(
            before,
            seen.len(),
            "a command name appears twice, and `find` would answer with \
             whichever was written first"
        );
    }

    #[test]
    fn every_name_is_typable() {
        for command in ALL {
            assert!(
                !command.name.is_empty()
                    && command
                        .name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-'),
                "`{}` is not a name somebody can type without thinking about it",
                command.name
            );
        }
    }

    #[test]
    fn every_summary_is_one_line_and_says_something() {
        for command in ALL {
            let help = command.help;
            assert!(!help.is_empty(), "`{}` has no summary", command.name);
            assert!(
                !help.contains('\n'),
                "`{}` has a summary spanning lines. A list of commands prints \
                 one line each; the long form belongs in docs/commands.md",
                command.name
            );
            assert!(
                help.len() <= 86,
                "`{}` has a {}-character summary, which will not sit beside a \
                 name in a list",
                command.name,
                help.len()
            );
            // A sentence, because it is read as one.
            assert!(
                help.ends_with('.'),
                "`{}`'s summary does not end a sentence",
                command.name
            );
        }
    }

    #[test]
    fn every_example_is_of_the_command_it_belongs_to() {
        // Whether the example *parses* is checked in ork-cli, against the real
        // parser. This is the cheaper half: that it is an example of the right
        // thing at all, which a copied line silently would not be.
        for command in ALL {
            let expected = format!("outlaw {}", command.name);
            assert!(
                command.usage.starts_with(&expected),
                "`{}` gives `{}` as its example, which is an example of \
                 something else",
                command.name,
                command.usage
            );
        }
    }

    #[test]
    fn every_command_has_a_section_in_the_manual() {
        // The table says a line each. Anything longer is the manual's, so a
        // command registered without a section there has no long form at all.
        let missing: Vec<&str> = ALL
            .iter()
            .map(|command| command.name)
            .filter(|name| !COMMANDS_PAGE.contains(&format!("## `outlaw {name}")))
            .collect();
        assert!(
            missing.is_empty(),
            "these commands are registered but have no section in \
             docs/commands.md: {missing:?}"
        );
    }

    #[test]
    fn the_manual_check_can_fail() {
        // The one above passes trivially if the page were ever empty or the
        // heading style changed. This is what says it is really looking.
        assert!(COMMANDS_PAGE.contains("## `outlaw scan"));
        assert!(!COMMANDS_PAGE.contains("## `outlaw nonesuch"));
    }

    #[test]
    fn every_group_has_something_in_it() {
        for group in Group::all() {
            assert!(
                in_group(group).next().is_some(),
                "the {} group is declared and empty, so it is a heading with \
                 nothing under it",
                group.heading()
            );
        }
        // And nothing is filed outside them.
        let grouped: usize = Group::all().iter().map(|g| in_group(*g).count()).sum();
        assert_eq!(grouped, ALL.len());
    }

    #[test]
    fn the_commands_that_can_change_the_machine_are_the_ones_we_think() {
        // Named individually rather than counted. A count would keep passing
        // while the wrong command was in the set, and this is the set that
        // decides what a read-only mode is allowed to run.
        let changing: Vec<&str> = ALL
            .iter()
            .filter(|command| command.changes.could_change_the_machine())
            .map(|command| command.name)
            .collect();
        assert_eq!(
            changing,
            vec!["processes", "fix"],
            "the set of commands that can change the machine has changed. If \
             that is right, say so here; it is not something to discover from \
             a list of differences"
        );
    }

    #[test]
    fn asking_about_a_command_that_is_not_there_says_so() {
        assert_eq!(help("scan"), "Run a scan and report what is wrong.");
        assert!(find("nonesuch").is_none());
        let missing = std::panic::catch_unwind(|| help("nonesuch"));
        assert!(
            missing.is_err(),
            "`help` answered for a command that does not exist, so a front-end \
             would show a blank line rather than fail"
        );
    }
}
