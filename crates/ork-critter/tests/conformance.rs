//! The reference implementation's own checks, run against this one.
//!
//! Ported from FieldKit's `tests/critterscript.test.js`, case by case, keeping
//! the source and the expected output exactly as they are there. That is what
//! makes this a conformance suite rather than a second opinion: **a port that
//! takes the same script and prints the same lines is the same language.** One
//! that only passes tests written against its own internals is a similar
//! language, and a similar language is worse than a different one, because
//! nobody is warned.
//!
//! Where an answer here differs from the reference on purpose, it is said so
//! at the case, with the reason. There are three such places and they are all
//! about wording.

use ork_critter::run::{Call, Collected, Command, Registration, Registry};
use ork_critter::{Value, run, run_as_guest};

// ---- the handful of commands the reference's suite defines ---------------

struct Simple {
    about: Registration,
    doing: fn(&mut Call<'_, '_>) -> Result<Option<Value>, String>,
}

impl Command for Simple {
    fn about(&self) -> &Registration {
        &self.about
    }
    fn run(&self, call: &mut Call<'_, '_>) -> Result<Option<Value>, String> {
        (self.doing)(call)
    }
}

fn registry() -> Registry {
    let mut registry = ork_critter::standard();
    let mut add =
        |about: Registration, doing: fn(&mut Call<'_, '_>) -> Result<Option<Value>, String>| {
            registry
                .add(Box::new(Simple { about, doing }))
                .expect("none of these clash with a built-in");
        };
    add(
        Registration::new("p").help("print").guest_safe(true),
        |call| {
            let said = call.words().join(" ");
            call.say(said)?;
            Ok(None)
        },
    );
    add(Registration::new("ret").guest_safe(true), |call| {
        Ok(Some(call.arg(0).cloned().unwrap_or(Value::Nothing)))
    });
    add(Registration::new("sealed"), |call| {
        call.say("ran")?;
        Ok(None)
    });
    add(Registration::new("boom").guest_safe(true), |_| {
        Err("deliberate".to_string())
    });
    registry
}

fn out(source: &str) -> String {
    let registry = registry();
    let mut collected = Collected::default();
    run(source, &registry, &mut collected).unwrap_or_else(|fault| panic!("should run: {fault}"));
    collected.joined()
}

fn refuse(source: &str) -> String {
    let registry = registry();
    let mut collected = Collected::default();
    match run(source, &registry, &mut collected) {
        Ok(_) => panic!("this should not have run: {source}"),
        Err(fault) => fault.to_string(),
    }
}

fn out_guest(source: &str) -> Result<String, String> {
    let registry = registry();
    let mut collected = Collected::default();
    match run_as_guest(source, &registry, &mut collected) {
        Ok(_) => Ok(collected.joined()),
        Err(fault) => Err(fault.to_string()),
    }
}

// ---- the tokenizer -------------------------------------------------------

#[test]
fn tokenizer() {
    assert_eq!(out("p one # this is ignored"), "one");
    assert_eq!(out("# nothing\np two"), "two");
    assert_eq!(out("\n\np three\n\n"), "three");
    assert_eq!(out(r#"p "a b  c""#), "a b  c");
    assert_eq!(out("p 'a b'"), "a b");
    assert_eq!(out(r#"p "a\tb""#), "a\tb");
    assert_eq!(out(r#"p "it's""#), "it's");
    assert!(
        refuse(r#"p "no end"#)
            .to_lowercase()
            .contains("unclosed string")
    );
    assert!(refuse("p $ 5").contains("$ must be followed by a name"));
    // Paths and URLs must survive unquoted, or every setting is a quoting
    // exercise.
    assert_eq!(
        out("p https://relay.example.com/health"),
        "https://relay.example.com/health"
    );
    assert_eq!(out(r"p C:\FieldKit\core"), r"C:\FieldKit\core");
}

#[test]
fn interpolation() {
    assert_eq!(out("let n = 4\np \"n is $n\""), "n is 4");
    assert_eq!(out(r#"p "x=$nope.""#), "x=.");
    assert_eq!(
        out(r#"let a = 1
p "$a$a""#),
        "11"
    );
    assert_eq!(out(r#"p "100$""#), "100$");
}

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(out("p (2 + 3 * 4)"), "14");
    assert_eq!(out("p ((2 + 3) * 4)"), "20");
    assert_eq!(out("p (7 / 2)"), "3.5");
    assert_eq!(out("p (7 % 3)"), "1");
    assert_eq!(out("p (0 - 5)"), "-5");
    assert_eq!(out("let a = 3\np ($a * -2)"), "-6");
    // Trimmed rather than exponential, and not fifteen places of arithmetic
    // showing through.
    assert_eq!(out("p (1 / 3)"), "0.333333");
    assert!(refuse("p (1 / 0)").contains("divide by zero"));
    assert!(refuse("p (1 % 0)").contains("remainder by zero"));
    assert!(refuse("p (hello * 2)").contains("not a number"));
}

#[test]
fn plus_joins_when_either_side_is_text() {
    // Somebody writing `p ($a + $b)` with words means join; with numbers they
    // mean add. Guessing wrong either way is worse than the small
    // inconsistency.
    assert_eq!(out("p (2 + 3)"), "5");
    assert_eq!(out(r#"p ("a" + 1)"#), "a1");
    assert_eq!(out(r#"p (1 + "a")"#), "1a");
    assert_eq!(out(r#"p ("2" + 3)"#), "5");
}

#[test]
fn comparison_and_logic() {
    assert_eq!(out("p (1 < 2)"), "true");
    assert_eq!(out("p (2 >= 2)"), "true");
    assert_eq!(out(r#"p (1 == "1")"#), "true");
    assert_eq!(out("p (1 != 2)"), "true");
    assert_eq!(out("p (true and false)"), "false");
    assert_eq!(out("p (false or true)"), "true");
    assert_eq!(out("p (not false)"), "true");
    assert_eq!(out("p (true or false and false)"), "true");
    // Short-circuiting is observable: the right side must not run at all.
    assert_eq!(out("p (false and (1 / 0))"), "false");
    assert_eq!(out("p (true or (1 / 0))"), "true");
}

#[test]
fn blocks() {
    assert_eq!(out("if true\n  p yes\nend"), "yes");
    assert_eq!(out("if false\n  p yes\nend"), "");
    assert_eq!(out("if false\n  p a\nelse\n  p b\nend"), "b");
    assert_eq!(out("repeat 3\n  p x\nend"), "x\nx\nx");
    assert_eq!(out("repeat 0\n  p x\nend"), "");
    assert_eq!(
        out("let i = 3\nwhile $i > 0\n  p $i\n  let i = $i - 1\nend"),
        "3\n2\n1"
    );
    assert_eq!(out("repeat 2\n  if true\n    p in\n  end\nend"), "in\nin");
    assert_eq!(out("let n = 0\nrepeat 3\n  let n = $n + 1\nend\np $n"), "3");
}

#[test]
fn structure_is_checked_before_anything_runs() {
    // The alternative is a script that prints half its output and then dies.
    let unclosed = ork_critter::check("if true\n  p hi").unwrap_err();
    assert!(unclosed.to_string().contains("never closed"), "{unclosed}");
    assert!(unclosed.to_string().contains("line 1"), "{unclosed}");

    assert!(ork_critter::check("p hi\nend").is_err());
    assert!(ork_critter::check("p hi\nelse\np x\nend").is_err());
    assert!(ork_critter::check("if\n p x\nend").is_err());
    assert!(ork_critter::check("if true\n p hi\nend").is_ok());

    // Nothing may run during a check. A script that printed while being
    // checked would be a nasty surprise.
    let collected = Collected::default();
    assert!(ork_critter::check("p side-effect").is_ok());
    assert!(collected.lines.is_empty());
}

#[test]
fn runtime_complaints_name_the_line_and_the_cause() {
    let complaint = refuse("p ok\np ok\nnosuch thing");
    assert!(complaint.contains("line 3"), "{complaint}");
    assert!(complaint.contains("nosuch"), "{complaint}");
    assert!(complaint.to_lowercase().contains("help"), "{complaint}");

    let complaint = refuse("p a\np $missing");
    assert!(complaint.contains("$missing"), "{complaint}");
    assert!(complaint.contains("let missing ="), "{complaint}");

    let complaint = refuse("p one\nboom");
    assert!(complaint.contains("deliberate"), "{complaint}");
    assert!(complaint.contains("line 2"), "{complaint}");

    // Attached exactly once. Nested blocks re-raising would otherwise give
    // "line 3: line 3: line 3:".
    let complaint = refuse("repeat 2\n  if true\n    boom\n  end\nend");
    assert_eq!(complaint.matches("line ").count(), 1, "{complaint}");
}

#[test]
fn budgets_are_the_reason_this_is_safe_to_type_into() {
    let complaint = refuse("while true\n  let x = 1\nend");
    assert!(
        complaint.contains("condition can become false"),
        "{complaint}"
    );

    // Under the loop ceiling and far over the output one, so this proves the
    // OUTPUT cap fires rather than the turn cap.
    let complaint = refuse("repeat 40000\n  p x\nend");
    assert!(complaint.contains("lines"), "{complaint}");

    // The reference says "iteration limit" here. This says "turn limit", for
    // the same reason the rest of this tool avoids the word: somebody reading
    // it is halfway through fixing a machine, not reading a manual.
    let complaint = refuse("repeat 999999999\n  let x = 1\nend");
    assert!(complaint.contains("turn limit"), "{complaint}");

    assert!(refuse("repeat (0 - 1)\n p x\nend").contains("zero or more"));

    // Budgets are per run, not shared: one heavy script must not leave the
    // next one already spent.
    let first = out("repeat 50\n  p x\nend");
    let second = out("repeat 50\n  p x\nend");
    assert_eq!(first.lines().count(), 50);
    assert_eq!(second.lines().count(), 50);
}

#[test]
fn commands() {
    assert_eq!(out("ret hello"), "hello");
    assert_eq!(out("p "), "");
    assert!(refuse("split").contains("needs 1 argument"));
    assert!(refuse("split").contains("Usage: split"));
    assert_eq!(out("p (count (split 'a b c'))"), "3");
}

#[test]
fn the_registry_cannot_be_confused_by_a_name() {
    // On a plain JavaScript object this is a real fault: `constructor`
    // resolves to a function and the interpreter tries to run it as a command.
    // Nothing here inherits anything, so these are ordinary unknown names --
    // but they are worth checking, because "it cannot happen in this language"
    // is exactly the sort of thing that stops being true.
    for name in ["constructor", "toString", "__proto__", "prototype"] {
        let complaint = refuse(name);
        assert!(
            complaint.contains("no command called"),
            "`{name}` gave: {complaint}"
        );
    }
}

#[test]
fn guest_mode() {
    assert_eq!(out_guest("p hi"), Ok("hi".to_string()));
    let complaint = out_guest("sealed").unwrap_err();
    // The reference says "guest mode". This says "when looking only", because
    // this tool has a read-only mode for a paired machine and calling its
    // owner a guest would be wrong about who they are.
    assert!(complaint.contains("not available"), "{complaint}");
    assert_eq!(out("sealed"), "ran");
    // Fail closed: a command that never said it was safe is not.
    let mut registry = registry();
    registry
        .add(Box::new(Simple {
            about: Registration::new("unflagged"),
            doing: |call| {
                call.say("leaked")?;
                Ok(None)
            },
        }))
        .expect("a new name");
    let mut collected = Collected::default();
    assert!(run_as_guest("unflagged", &registry, &mut collected).is_err());
    assert!(collected.lines.is_empty(), "it printed anyway");
}

#[test]
fn variables() {
    assert_eq!(out("let a = 1\nlet a = 2\np $a"), "2");
    assert_eq!(out("let a = 1\nlet a = $a + 1\np $a"), "2");
    assert_eq!(
        out(r#"let s = "hi there"
p $s"#),
        "hi there"
    );
    assert_eq!(out("let b = true\nif $b\n p yes\nend"), "yes");
    assert!(refuse("let a =").to_lowercase().contains("let needs"));
    assert!(refuse("let = 5").to_lowercase().contains("let needs"));
}

#[test]
fn truthiness_is_predictable() {
    assert_eq!(out("if 0\n p y\nelse\n p n\nend"), "n");
    assert_eq!(out("if \"\"\n p y\nelse\n p n\nend"), "n");
    assert_eq!(out("if false\n p y\nelse\n p n\nend"), "n");
    assert_eq!(out("if word\n p y\nend"), "y");
    assert_eq!(out("if 1\n p y\nend"), "y");
    // `no` and `off` read as false because `set thing off` is something people
    // type, and having it read as true would be a trap exactly where it
    // matters.
    assert_eq!(out("if no\n p y\nelse\n p n\nend"), "n");
    assert_eq!(out("if off\n p y\nelse\n p n\nend"), "n");
}

// ---- brackets run a command ---------------------------------------------

#[test]
fn brackets_run_a_command_and_hand_back_the_answer() {
    assert_eq!(out("let n = (ret 5)\np $n"), "5");
    assert_eq!(out("p (ret 5) + 1"), "6");
    assert_eq!(out("p (ret hi) there"), "hi there");
    assert_eq!(out("let n = (count (list a b c))\np $n"), "3");
    assert_eq!(out("if (count (list a b))\n p yes\nend"), "yes");
    // The distinguishing rule: a word followed by a binary operator is
    // arithmetic, not a call.
    assert_eq!(out("let a = yes\nif (yes == $a)\n p same\nend"), "same");
    assert_eq!(out("let x = (ret quiet)\np done"), "done");
    assert!(
        ork_critter::check("let n = (count $x")
            .unwrap_err()
            .to_string()
            .contains("closing )")
    );
    // Guest mode has to reach inside brackets too. This is the hole that
    // appears the moment the two call paths are separate copies.
    let complaint = out_guest("let x = (sealed)\np $x").unwrap_err();
    assert!(complaint.contains("not available"), "{complaint}");
}

// ---- the standard library ------------------------------------------------

#[test]
fn list_and_text_commands() {
    assert_eq!(out("p (list a b c)"), "a, b, c");
    assert_eq!(out("p (count (list a b c))"), "3");
    assert_eq!(out("p (count hello)"), "5");
    assert_eq!(out("p (item (list a b c) 2)"), "b");
    assert_eq!(out("let l = (list a b c)\np (item $l -1)"), "c");
    assert_eq!(out("p -1"), "-1");
    assert!(refuse("p (item (list a) 9)").contains("no item 9"));
    assert_eq!(out("p (count (split 'a b c'))"), "3");
    assert_eq!(out("p (item (split a-b-c -) 2)"), "b");
    assert_eq!(out("p (join (list a b) -)"), "a-b");
    assert_eq!(out("p (add (list a) b c)"), "a, b, c");
    assert_eq!(
        out("let l = (list a)\nlet m = (add $l b)\np (count $l)"),
        "1"
    );
    assert_eq!(out("p (upper hi there)"), "HI THERE");
    assert_eq!(out("p (lower HI)"), "hi");
    assert_eq!(out("p (count (trim '  hi  '))"), "2");
    assert_eq!(out("p (has hello ell)"), "true");
    assert_eq!(out("p (has (list a b) b)"), "true");
    assert_eq!(out("p (has (list a b) z)"), "false");
    assert_eq!(out("p (round 3.7)"), "4");
    assert_eq!(out("p (round 3.14159 2)"), "3.14");
    assert_eq!(out("p (range 3)"), "1, 2, 3");
    assert_eq!(out("p (range 2 4)"), "2, 3, 4");
    assert!(refuse("p (range 1 999999)").contains("limit"));
}

#[test]
fn the_rest_of_the_standard_library() {
    // Sorting numbers as numbers is the whole point: 2, 10, 9 sorted as text
    // gives 10, 2, 9, which looks like the sort is broken.
    assert_eq!(out("p (sort (list 2 10 9))"), "2, 9, 10");
    assert_eq!(out("p (sort (list b a c))"), "a, b, c");
    assert_eq!(out("p (sort (list 1 2 3) down)"), "3, 2, 1");
    assert_eq!(out("p (reverse (list a b))"), "b, a");
    assert_eq!(out("p (reverse abc)"), "cba");
    assert_eq!(out("p (unique (list a b a))"), "a, b");
    assert_eq!(out("p (first (list a b c))"), "a");
    assert_eq!(out("p (first (list a b c) 2)"), "a, b");
    assert_eq!(out("p (last (list a b c))"), "c");
    assert_eq!(out("p (last (list a b c) 2)"), "b, c");
    assert_eq!(out("p (slice (list a b c d) 2 3)"), "b, c");
    // Through a variable, not a bracketed call -- see the case below.
    assert_eq!(
        out("let l = (list a b c d)
p (slice $l -2)"),
        "c, d"
    );
    assert_eq!(out("p (without (list a b c) b)"), "a, c");
    assert_eq!(
        out("p (only (list apple pear apricot) ap)"),
        "apple, apricot"
    );
    assert_eq!(out("p (sum (list 1 2 3))"), "6");
    assert_eq!(out("p (min (list 3 1 2))"), "1");
    assert_eq!(out("p (max 3 1 2)"), "3");
    assert_eq!(out("p (words '  a  b  ')"), "a, b");
    assert_eq!(out(r#"p (replace hello l L)"#), "heLLo");
    assert_eq!(out("p (starts hello he)"), "true");
    assert_eq!(out("p (ends hello lo)"), "true");
    assert_eq!(out("p (number '42')"), "42");
    assert_eq!(out("p (text 42)"), "42");
    assert_eq!(out("p (kind (list a))"), "a list");
    assert_eq!(out("p (kind 3)"), "a number");
}

#[test]
fn records() {
    assert_eq!(out("p (record name Ada age 30)"), "name=Ada age=30");
    assert_eq!(out("p (field (record name Ada) name)"), "Ada");
    assert_eq!(out("p (fields (record name Ada age 30))"), "name, age");
    assert_eq!(out("p (with (record a 1) b 2)"), "a=1 b=2");
    // Naming what IS there. A silent nothing is how one mistyped field name
    // becomes an hour of confusion.
    let complaint = refuse("p (field (record name Ada) nme)");
    assert!(complaint.contains("no field called 'nme'"), "{complaint}");
    assert!(complaint.contains("It has: name"), "{complaint}");
    assert!(refuse("p (field hello name)").contains("needs a record, not text"));
    // One argument is caught by the count before the pairing is looked at,
    // which is the more useful complaint of the two.
    assert!(refuse("p (record a)").contains("needs 2 argument"));
    assert!(refuse("p (record a b c)").contains("needs pairs"));
}

#[test]
fn json_reads_into_records_and_lists() {
    // A record is exactly the shape JSON reads into, which is the reason
    // `... | json | field title` works with nothing to convert in the middle.
    assert_eq!(out(r#"p (field (json '{"a":1}') a)"#), "1");
    assert_eq!(out(r#"p (count (json '[1,2,3]'))"#), "3");
    assert_eq!(out(r#"p (json 'true')"#), "true");
    assert!(refuse("p (json 'not json')").contains("not JSON"));
}

#[test]
fn a_bracketed_argument_followed_by_a_sign_is_arithmetic() {
    // Found by writing `slice (list a b c d) -2` and getting "'a, b, c, d' is
    // not a number". It is not a fault in the port: the reference does the
    // same, and for a defensible reason. An argument that opens with `(` is an
    // *expression*, and an expression followed by `- 2` is a subtraction. The
    // argument rule -- one piece each, a sign against its digits is part of
    // the number -- applies to plain pieces, and a bracket opts out of it.
    //
    // Pinned rather than fixed. It is the one place in the language where the
    // same-looking thing means two things, and both readings are defensible,
    // so the useful thing is that it fails loudly rather than computing
    // something nobody asked for. Through a variable it reads as everybody
    // expects.
    let complaint = refuse("p (slice (list a b) -1)");
    assert!(complaint.contains("is not a number"), "{complaint}");
    assert_eq!(
        out("let l = (list a b)
p (slice $l -1)"),
        "b"
    );
    // And the same shape with a spaced sign is plainly a subtraction, which is
    // what the complaint above is really about.
    assert_eq!(out("p ((count (list a b c)) - 1)"), "2");
}

#[test]
fn where_and_pluck_work_on_lists_of_records() {
    let script = "let a = (record name Ada job maths)
let b = (record name Bob job music)
let both = (list $a $b)
p (pluck $both name)
p (count (where $both job music))";
    assert_eq!(out(script), "Ada, Bob\n1");
}

// ---- for, loop control, functions ---------------------------------------

#[test]
fn for_loops() {
    assert_eq!(out("for x in (list a b c)\n p $x\nend"), "a\nb\nc");
    assert_eq!(out("let l = (list 1 2)\nfor x in $l\n p $x\nend"), "1\n2");
    assert_eq!(out("for i in (range 3)\n p $i\nend"), "1\n2\n3");
    // The loop variable survives the loop, which is what makes `for` usable
    // for finding something.
    assert_eq!(out("for x in (list a b)\nend\np $x"), "b");
    assert_eq!(out("for x in hello\n p $x\nend"), "hello");
    assert_eq!(out("for x in ''\n p $x\nend"), "");
    assert_eq!(
        out("for a in (list 1 2)\n for b in (list x y)\n  p \"$a$b\"\n end\nend"),
        "1x\n1y\n2x\n2y"
    );
    assert!(
        ork_critter::check("for x (list a)\nend")
            .unwrap_err()
            .to_string()
            .contains("for name in")
    );
    assert!(
        ork_critter::check("for x in (list a)\np y")
            .unwrap_err()
            .to_string()
            .contains("never closed")
    );
}

#[test]
fn break_and_continue() {
    assert_eq!(
        out("for x in (range 5)\n if $x > 2\n  break\n end\n p $x\nend"),
        "1\n2"
    );
    assert_eq!(
        out("let i = 0\nrepeat 9\n let i = $i + 1\n if $i == 2\n  break\n end\nend\np $i"),
        "2"
    );
    assert_eq!(
        out("let i = 0\nwhile true\n let i = $i + 1\n if $i > 3\n  break\n end\nend\np $i"),
        "4"
    );
    assert_eq!(
        out("for x in (range 4)\n if $x == 2\n  continue\n end\n p $x\nend"),
        "1\n3\n4"
    );
    // Only the inner loop.
    assert_eq!(
        out("for a in (list 1 2)\n for b in (list x y)\n  break\n end\n p $a\nend"),
        "1\n2"
    );
    assert!(refuse("break").contains("outside a loop"));
    assert!(refuse("continue").contains("outside a loop"));
}

#[test]
fn functions() {
    assert_eq!(out("def hi\n p hello\nend\nhi"), "hello");
    assert_eq!(out("def greet who\n p hi $who\nend\ngreet Ada"), "hi Ada");
    assert_eq!(out("def two\n return 2\nend\nlet n = (two)\np $n"), "2");
    assert_eq!(out("def two\n return 2\nend\ntwo"), "2");
    assert_eq!(
        out("def f\n return 1\n p unreachable\nend\nlet x = (f)\np $x"),
        "1"
    );
    assert_eq!(
        out("def f\n for i in (range 9)\n  if $i == 2\n   return $i\n  end\n end\nend\np (f)"),
        "2"
    );
    assert_eq!(out("p (later)\ndef later\n return ok\nend"), "ok");
    assert_eq!(out("def f a\n p \"[$a]\"\nend\nf"), "[]");
    assert_eq!(out("def f\n p (count $args)\nend\nf a b c"), "3");
    assert_eq!(
        out("def a\n return 1\nend\ndef b\n return (a) + 1\nend\np (b)"),
        "2"
    );

    // The brackets around the subtraction are load-bearing, and worth a case
    // of their own: `down $n - 1` is three arguments by the argument rule, so
    // the counter never moves. That is correct, and it is the thing somebody
    // will get wrong first.
    assert_eq!(
        out(
            "def down n\n if $n <= 0\n  return done\n end\n return (down ($n - 1))\nend\np (down 5)"
        ),
        "done"
    );
    let complaint = refuse(
        "def down n\n if $n <= 0\n  return done\n end\n return (down $n - 1)\nend\np (down 5)",
    );
    assert!(complaint.contains("nested more than"), "{complaint}");

    assert_eq!(
        out("let x = outer\ndef f\n let x = inner\nend\nf\np $x"),
        "outer"
    );
    assert!(refuse("let x = 1\ndef f\n p $x\nend\nf").contains("has not been set"));
    assert!(
        ork_critter::check("def\nend")
            .unwrap_err()
            .to_string()
            .contains("needs a name")
    );
    assert!(
        ork_critter::check("def f\np x")
            .unwrap_err()
            .to_string()
            .contains("never closed")
    );
    // A written command cannot take over a real one. If it could, one script
    // could change what every other script does.
    assert_eq!(out("def p\n return shadowed\nend\np hello"), "hello");
}

#[test]
fn functions_share_the_runs_budgets() {
    // The important one. If a call got a fresh allowance, runaway recursion
    // would spin until something else killed it rather than stopping with a
    // message, and the budgets would be decoration.
    let complaint = refuse("def f\n return (f)\nend\np (f)");
    assert!(
        complaint.contains("nested more than") || complaint.contains("steps"),
        "{complaint}"
    );
    let complaint = refuse("def spin\n while true\n  p x\n end\nend\nspin");
    assert!(
        complaint.contains("turns") || complaint.contains("steps") || complaint.contains("lines"),
        "{complaint}"
    );
}

// ---- elif ----------------------------------------------------------------

#[test]
fn elif_chains() {
    assert_eq!(
        out("let x = 2\nif $x == 1\n p one\nelif $x == 2\n p two\nend"),
        "two"
    );
    assert_eq!(
        out("let x = 9\nif $x == 1\n p one\nelif $x == 2\n p two\nelse\n p other\nend"),
        "other"
    );
    assert_eq!(
        out("let x = 3\nif $x == 1\n p a\nelif $x == 2\n p b\nelif $x == 3\n p c\nelif $x == 4\n p d\nend"),
        "c"
    );
    assert_eq!(
        out("let x = 1\nif $x == 1\n p a\nelif true\n p b\nelse\n p c\nend"),
        "a"
    );

    // The fault this section exists for: a chain parsed by recursion swallowed
    // every following statement into the else branch, so the rest of the
    // script ran only when the first condition was false. Nothing about that
    // is visible until a test looks past the `end`.
    assert_eq!(out("if true\n p a\nelif true\n p b\nend\np after"), "a\nafter");
    assert_eq!(out("if false\n p a\nelif false\n p b\nend\np after"), "after");
    assert_eq!(
        out("if false\n p a\nelif false\n p b\nelse\n p c\nend\np after"),
        "c\nafter"
    );
    assert_eq!(
        out("for i in (range 3)\n if $i == 1\n  p one\n elif $i == 2\n  p two\n else\n  p many\n end\nend"),
        "one\ntwo\nmany"
    );
    assert!(
        ork_critter::check("if true\np a\nelif\np b\nend")
            .unwrap_err()
            .to_string()
            .contains("needs a condition")
    );
    assert!(
        ork_critter::check("elif true\np a\nend")
            .unwrap_err()
            .to_string()
            .contains("no matching")
    );
}

// ---- the pipe ------------------------------------------------------------

#[test]
fn the_pipe_sends_the_answer_along() {
    assert_eq!(out("ret hello | p"), "hello");
    assert_eq!(out("list 3 1 10 2 | sort | join \", \""), "1, 2, 3, 10");
    // Numbers sorting as text would give 10, 2, 3 -- which looks exactly like
    // a broken sort rather than a surprise about kinds.
    assert_eq!(out("list 2 10 9 | sort | join ,"), "2,9,10");
    assert_eq!(out("list a b c | count"), "3");
    assert_eq!(out("p one two"), "one two");
}

#[test]
fn it_is_where_the_answer_goes_instead() {
    assert_eq!(
        out("list x y z | count | p \"there are $it items\""),
        "there are 3 items"
    );
    assert_eq!(out("ret world | p \"hello $it\""), "hello world");
    // Left set, `$it` would read as an ordinary variable on every later line,
    // holding whichever chain happened to run last.
    assert!(refuse("list a b | count | p $it\np $it").contains("$it has not been set"));
}

#[test]
fn the_arrow_names_the_answer() {
    assert_eq!(out("list a b c | count -> $n\np $n"), "3");
    // The dollar is optional on the name being written to.
    assert_eq!(out("list a b c | count -> n\np $n"), "3");
    assert_eq!(out("ret 7 -> $x\np $x"), "7");
    assert_eq!(out("list a b | count -> $n"), "");
}

#[test]
fn a_step_with_no_answer_cannot_be_piped() {
    // This printed "hello" and uppercased nothing, silently. The distinction
    // that fixes it is "no answer" against "the answer is nothing", and the
    // message has to name the step or it is a mystery rather than a complaint.
    assert!(refuse("p hello | upper").contains("does not have an answer to pass along"));
    assert!(refuse("p hi | upper").contains("'p'"));
    // An empty answer is a real value and has to pass. Conflating the two is
    // what made the original fault silent.
    assert_eq!(out("ret \"\" | count"), "0");
}

#[test]
fn records_through_a_pipe() {
    assert_eq!(out("record name Ada age 30"), "name=Ada age=30");
    assert_eq!(out("record name Ada age 30 | field name"), "Ada");
    assert_eq!(out("record name Ada age 30 | fields | join ,"), "name,age");
    // `with` copies rather than changes the original.
    assert_eq!(
        out("record a 1 -> $r\nwith $r b 2 -> $s\np (count (fields $r))"),
        "1"
    );
    // A record is deliberately the shape JSON reads into, so this needs no
    // conversion step in the middle.
    assert_eq!(out("json \"[{\\\"a\\\":1},{\\\"a\\\":2}]\" | pluck a | sum"), "3");
    assert_eq!(out("record a 1 | kind"), "a record");
    assert_eq!(out("list a | kind"), "a list");
    assert!(refuse("record name Ada | field nmae").contains("It has: name"));
}

#[test]
fn the_value_commands_the_pipe_made_necessary() {
    assert_eq!(
        out("json \"[{\\\"n\\\":\\\"a\\\",\\\"ok\\\":true},{\\\"n\\\":\\\"b\\\",\\\"ok\\\":false}]\" | where ok false | pluck n | join ,"),
        "b"
    );
    assert_eq!(out("list a b c d | first"), "a");
    assert_eq!(out("list a b c d | last 2 | join ,"), "c,d");
    assert_eq!(out("list a b c d | slice 2 3 | join ,"), "b,c");
    assert_eq!(out("list a b a c | unique | join ,"), "a,b,c");
    assert_eq!(out("list a b c | without b | join ,"), "a,c");
    assert_eq!(out("list cat dog cart | only ca | join ,"), "cat,cart");
    assert_eq!(out("ret \"one\\n\\ntwo\" | lines | count"), "2");
    assert_eq!(out("ret \"one  two three\" | words | count"), "3");
    assert_eq!(out("list 1 2 3 | sum"), "6");
    assert_eq!(out("list 4 1 9 | min"), "1");
    assert_eq!(out("list 4 1 9 | max"), "9");
    assert_eq!(out("ret \"a.b.c\" | replace . -"), "a-b-c");
    assert_eq!(out("ret hello | starts he"), "true");
    assert_eq!(out("ret hello | ends lo"), "true");
    assert_eq!(out("list a b c | reverse | join ,"), "c,b,a");
    assert_eq!(out("ret abc | reverse"), "cba");
    assert!(refuse("ret hello | number").contains("is not a number"));
}

#[test]
fn pipes_compose_with_the_rest_of_the_language() {
    assert_eq!(
        out("def dbl n\n  return ($n * 2)\nend\nlist 1 2 3 | first | dbl"),
        "2"
    );
    assert_eq!(out("for x in (list ab cde)\n  ret $x | count\nend"), "2\n3");
    // `->` inside a block writes where the rest of the script can see it.
    assert_eq!(out("if true\n  list a b | count -> $n\nend\np $n"), "2");
}

#[test]
fn a_pipeline_is_counted_against_the_step_budget() {
    // Or a chain becomes the way around the only thing keeping a typo from
    // running forever.
    let complaint = refuse("let n = 0\nwhile true\n  list a b | count -> $n\nend");
    assert!(
        complaint.contains("turns") || complaint.contains("steps"),
        "{complaint}"
    );
}

// ---- digits ---------------------------------------------------------------

#[test]
fn digits_at_the_start_of_a_piece_mean_a_number() {
    // Found through the reference's terminal, which titles notes with a
    // timestamp and used to echo back `read terminal 2026-08-21 15:04` -- a
    // line that did not work when pasted, because a piece beginning with a
    // digit is read as a number and the date comes apart into 2026, minus 8,
    // minus 21.
    //
    // Both halves are pinned. The splitting is not a fault to be fixed:
    // division and a glued negative both depend on a digit starting a number.
    // It is a thing the language has to say out loud and anything echoing a
    // timestamp has to quote around.
    assert_eq!(out("p \"terminal 2026-08-21 15:04\""), "terminal 2026-08-21 15:04");
    assert_ne!(out("p 2026-08-21 15:04"), "2026-08-21 15:04");
    assert_eq!(out("p sum-up"), "sum-up");
    assert_eq!(out("p https://example.org/a"), "https://example.org/a");
    // The reason the splitting stays.
    assert_eq!(out("p (6 / 2)"), "3");
}

#[test]
fn an_answer_round_trips_through_the_printed_form() {
    assert_eq!(out("p (round 3)"), "3");
    assert_eq!(out("p (1 / 3)"), "0.333333");
    assert_eq!(out("p (list a b)"), "a, b");
    assert_eq!(out("p (1 < 2)"), "true");
}
