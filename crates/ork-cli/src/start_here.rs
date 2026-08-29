//! What `outlaw`, typed on its own, says.
//!
//! It used to say `error: a subcommand is required`, print a wall of usage,
//! and exit 2. That is a reasonable default for a program whose users are all
//! already at a prompt, and this one's are not: somebody whose computer is
//! misbehaving has been told to run a repair tool, and the first thing it did
//! was call them wrong.
//!
//! So the bare command is treated as a question -- *what is this and what do I
//! type* -- and answered. Six commands, not thirty; `--help` is still there
//! for the rest. It also says how to open the window, because a person who
//! would rather click than type has no way of finding that out from a
//! terminal, and the shortcut that would have told them is the thing they did
//! not find.

use crate::style::{bold, dim};

/// The handful worth knowing before the manual.
///
/// Deliberately short. A first screen listing twenty commands is a reference
/// card, and somebody reads a reference card when they already know what they
/// are doing.
const FIRST_THINGS: &[(&str, &str)] = &[
    ("outlaw scan", "look for problems now"),
    (
        "outlaw fix",
        "work through what it found, asking before each change",
    ),
    ("outlaw watch", "keep looking, and say only what changed"),
    ("outlaw probes", "every check this build can run"),
    ("outlaw docs", "the manual, carried inside this program"),
    ("outlaw --help", "everything else"),
];

pub fn print() {
    println!();
    println!(
        "{} {}",
        bold(ork_core::ways_in::WINDOW_LABEL),
        dim(env!("CARGO_PKG_VERSION"))
    );
    println!(
        "  {}",
        dim("Finds hardware and software problems and says what they mean, in plain words.")
    );
    println!();

    println!("{}", bold("Try one of these"));
    for (command, what) in FIRST_THINGS {
        println!("  {command:<18}{}", dim(what));
    }
    println!();

    println!("{}", bold("If you would rather click than type"));
    match ork_core::ways_in::find_window() {
        Some(window) => {
            println!("  The window is installed:");
            println!("    {}", window.display());
            println!(
                "  {}",
                dim("It is also in your Start menu or applications list as \"Outlaw Repair Kit\".")
            );
        }
        None => {
            // Said carefully. Not finding it is not the same as it not being
            // there, and a diagnostic tool that is confidently wrong about
            // somebody's own computer has undermined the only thing it sells.
            println!(
                "  {}",
                dim("The window is a separate download, and was not found in the usual places:")
            );
            println!(
                "    {}",
                dim("https://github.com/Sup095/outlaw-repair-kit/releases/latest")
            );
            println!("  {}", dim("`outlaw docs install` explains both ways in."));
        }
    }
    println!();

    println!(
        "  {}",
        dim("Nothing here changes your computer until you ask it to, one change at a time.")
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_offered_is_one_the_program_actually_has() {
        // The failure this guards is a first screen that tells a person to
        // type something that does not work, which is the single worst thing
        // an orientation page can do. Checked against clap's own command list
        // rather than a copy of it, so renaming a command breaks this.
        use clap::CommandFactory;
        let cli = crate::Cli::command();
        let known: Vec<String> = cli
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .collect();

        for (line, _) in FIRST_THINGS {
            let word = line.split_whitespace().nth(1).unwrap();
            if word.starts_with("--") {
                // Actually run it through the parser rather than looking it
                // up in a list. Clap adds `--help` while building the command
                // rather than declaring it, so looking for it in the declared
                // arguments finds nothing and proves nothing.
                let outcome = crate::Cli::command().try_get_matches_from(["outlaw", word]);
                let kind = outcome
                    .expect_err("`--help` should print rather than parse")
                    .kind();
                assert!(
                    matches!(
                        kind,
                        clap::error::ErrorKind::DisplayHelp
                            | clap::error::ErrorKind::DisplayVersion
                    ),
                    "`{line}` is offered but `{word}` does not work: {kind:?}"
                );
                continue;
            }
            assert!(
                known.contains(&word.to_string()),
                "`{line}` is offered but `{word}` is not a command; known: {known:?}"
            );
        }
    }

    #[test]
    fn the_first_screen_stays_short_enough_to_read() {
        assert!(
            FIRST_THINGS.len() <= 8,
            "{} entries is a reference card, not a first screen",
            FIRST_THINGS.len()
        );
    }

    #[test]
    fn printing_it_does_not_need_anything_to_be_installed() {
        // It runs on a machine with no window, no settings, and no state
        // directory -- which is every machine the first time.
        print();
    }
}
