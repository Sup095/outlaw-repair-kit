//! The language as somebody writes it: source in, printed lines out.
//!
//! Shaped after FieldKit's `tests/critterscript.test.js`, which tests the
//! reference implementation the same way. That shape is the point rather than
//! a convenience: a port that passes the same source through and gets the same
//! lines back is the *same language*, and one that only passes tests written
//! against its own internals is merely a similar one.
//!
//! The command set here is deliberately tiny and belongs to the tests. Nothing
//! in this crate knows anything about repairing a machine, so nothing here
//! needs a machine to run against.

use ork_critter::run::{Call, Collected, Command, Registration, Registry};
use ork_critter::{Value, run, value::Record};

// ---- a few commands to say things with -----------------------------------

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
    let mut registry = Registry::new();
    let mut add = |about: Registration, doing: fn(&mut Call) -> Result<Option<Value>, String>| {
        registry
            .add(Box::new(Simple { about, doing }))
            .expect("these names are all different");
    };

    // Prints its arguments and answers nothing at all.
    add(
        Registration::new("p").help("print").guest_safe(true),
        |call| {
            let said = call.words().join(" ");
            call.say(said)?;
            Ok(None)
        },
    );
    // Answers its first argument without printing it.
    add(Registration::new("ret").guest_safe(true), |call| {
        Ok(Some(call.arg(0).cloned().unwrap_or(Value::Nothing)))
    });
    // Answers nothing, as a value. Not the same as answering nothing at all.
    add(Registration::new("nowt").guest_safe(true), |_| {
        Ok(Some(Value::Nothing))
    });
    add(Registration::new("upper").guest_safe(true), |call| {
        Ok(Some(Value::Text(
            call.arg(0)
                .map(Value::show)
                .unwrap_or_default()
                .to_uppercase(),
        )))
    });
    add(Registration::new("count").guest_safe(true), |call| {
        let how_many = match call.arg(0) {
            Some(Value::List(items)) => items.len(),
            Some(Value::Nothing) | None => 0,
            Some(_) => 1,
        };
        Ok(Some(Value::Num(how_many as f64)))
    });
    add(Registration::new("list").guest_safe(true), |call| {
        Ok(Some(Value::List(call.args.clone())))
    });
    add(
        Registration::new("split").guest_safe(true).min_args(1),
        |call| {
            let text = call.arg(0).map(Value::show).unwrap_or_default();
            let by = call.arg(1).map(Value::show).unwrap_or_else(|| " ".into());
            Ok(Some(Value::List(
                text.split(&by)
                    .map(|part| Value::Text(part.into()))
                    .collect(),
            )))
        },
    );
    add(Registration::new("person").guest_safe(true), |_| {
        let mut record = Record::new();
        record.set("name", Value::Text("Ada".into()));
        record.set("age", Value::Num(30.0));
        Ok(Some(Value::Record(record)))
    });
    // Not for somebody who is only allowed to look.
    add(Registration::new("sealed"), |call| {
        call.say("ran")?;
        Ok(None)
    });
    add(Registration::new("boom").guest_safe(true), |_| {
        Err("deliberate".to_string())
    });
    registry
}

/// Run a script and collect what it printed.
fn out(source: &str) -> String {
    let registry = registry();
    let mut collected = Collected::default();
    run(source, &registry, &mut collected).unwrap_or_else(|fault| panic!("should run: {fault}"));
    collected.joined()
}

/// Run a script that should not work, and give back the complaint.
fn refuse(source: &str) -> String {
    let registry = registry();
    let mut collected = Collected::default();
    match run(source, &registry, &mut collected) {
        Ok(_) => panic!("this should not have run: {source}"),
        Err(fault) => fault.to_string(),
    }
}

fn out_as_guest(source: &str) -> Result<String, String> {
    let registry = registry();
    let mut collected = Collected::default();
    match ork_critter::run::run_as_guest(source, &registry, &mut collected) {
        Ok(_) => Ok(collected.joined()),
        Err(fault) => Err(fault.to_string()),
    }
}

// ---- rule one ------------------------------------------------------------

#[test]
fn a_bare_word_is_a_string_and_a_dollar_name_is_a_variable() {
    assert_eq!(out("p hello"), "hello");
    assert_eq!(out("p hello world again"), "hello world again");
    assert_eq!(out("let x = 7\np $x"), "7");
    // The half that matters most: a word that looks like a variable name is
    // still a word.
    assert_eq!(out("let name = Ada\np name"), "name");
}

#[test]
fn words_that_are_keywords_elsewhere_are_just_words() {
    // Somebody typing `p null` should not get a surprise because another
    // language would have.
    assert_eq!(
        out("p null undefined NaN class function"),
        "null undefined NaN class function"
    );
}

#[test]
fn a_string_has_its_variables_put_in_when_it_runs() {
    assert_eq!(
        out(r#"let n = 3
p "there are $n of them""#),
        "there are 3 of them"
    );
}

#[test]
fn a_name_that_is_not_set_becomes_nothing_inside_a_string() {
    // The alternative makes every "$path/$file" a landmine.
    assert_eq!(out(r#"p "[$nothing]""#), "[]");
}

#[test]
fn a_name_that_is_not_set_is_a_complaint_outside_a_string() {
    // Outside quotes it is almost always a typo, and saying so is the whole
    // value of having variables be marked at all.
    let complaint = refuse("p $missing");
    assert!(
        complaint.contains("$missing has not been set"),
        "{complaint}"
    );
    assert!(complaint.contains("let missing ="), "{complaint}");
}

// ---- rules two and three -------------------------------------------------

#[test]
fn the_answer_becomes_the_first_argument_of_the_next_step() {
    assert_eq!(out("ret hello | upper"), "HELLO");
}

#[test]
fn a_step_that_mentions_it_gets_the_answer_there_instead() {
    // And nothing is prepended, so it is not said twice.
    assert_eq!(out(r#"ret world | p "hello $it""#), "hello world");
}

#[test]
fn an_arrow_names_the_answer_rather_than_printing_it() {
    assert_eq!(out("ret 5 | upper -> $x\np $x"), "5");
    // And the pipeline itself printed nothing.
    assert_eq!(out("ret 5 -> $x"), "");
}

#[test]
fn a_pipeline_reads_in_the_order_the_work_happens() {
    assert_eq!(out(r#"ret "a b c" | split " " | count"#), "3");
}

#[test]
fn a_step_with_no_answer_cannot_be_piped_from() {
    // Without this, `p hello | upper` quietly prints "hello" and then
    // uppercases an empty string, which looks like `upper` is broken.
    let complaint = refuse("p hello | upper");
    assert!(
        complaint.contains("does not have an answer to pass along"),
        "{complaint}"
    );
    assert!(complaint.contains("'p'"), "{complaint}");
}

#[test]
fn nothing_is_an_answer_and_may_be_passed_along() {
    // "no answer" and "the answer is nothing" are different things, and only
    // the first one is a mistake at a pipe.
    assert_eq!(out("nowt | count"), "0");
}

#[test]
fn it_belongs_to_the_pipeline_and_not_to_the_script() {
    // Left set afterwards it would read as an ordinary variable on every later
    // line -- and a stale one, holding whichever chain ran last.
    let complaint = refuse("ret 5 | upper\np $it");
    assert!(complaint.contains("$it has not been set"), "{complaint}");
}

#[test]
fn a_pipeline_inside_an_argument_does_not_eat_the_outer_ones_it() {
    assert_eq!(
        out(r#"ret outer | p "$it and (upper inner)""#),
        "outer and (upper inner)"
    );
}

// ---- values --------------------------------------------------------------

#[test]
fn a_record_prints_its_fields() {
    assert_eq!(out("person"), "name=Ada age=30");
}

#[test]
fn a_statement_prints_its_answer_and_an_expression_keeps_it() {
    // The only difference between the two forms.
    assert_eq!(out("ret hello"), "hello");
    assert_eq!(out("let x = (ret hello)"), "");
    assert_eq!(out("let x = (ret hello)\np $x"), "hello");
}

#[test]
fn arithmetic_works_the_way_it_looks() {
    assert_eq!(out("p (1 + 2 * 3)"), "7");
    assert_eq!(out("p ((1 + 2) * 3)"), "9");
    assert_eq!(out("p (10 / 4)"), "2.5");
    assert_eq!(out("p (10 % 3)"), "1");
}

#[test]
fn plus_joins_text_and_adds_numbers() {
    // What somebody writing `say ($a + $b)` almost always means.
    assert_eq!(out(r#"p ("a" + "b")"#), "ab");
    assert_eq!(out("p (2 + 3)"), "5");
    assert_eq!(out(r#"p ("2" + 3)"#), "5");
}

#[test]
fn dividing_by_zero_says_so_rather_than_answering_infinity() {
    assert!(refuse("p (1 / 0)").contains("divide by zero"));
    assert!(refuse("p (1 % 0)").contains("remainder by zero"));
}

#[test]
fn comparing_a_number_with_the_same_number_typed_works() {
    assert_eq!(out("if (3 == \"3\")\np same\nend"), "same");
}

#[test]
fn and_and_or_stop_as_soon_as_they_know() {
    // The right side of an `and` whose left is false must not be looked at:
    // `$missing` would otherwise be a complaint rather than a skipped branch.
    assert_eq!(out("if (false and $missing)\np no\nelse\np ok\nend"), "ok");
    assert_eq!(out("if (true or $missing)\np ok\nend"), "ok");
}

// ---- blocks --------------------------------------------------------------

#[test]
fn an_if_chain_runs_only_the_branch_that_matches() {
    let script =
        "let n = 2\nif ($n == 1)\np one\nelif ($n == 2)\np two\nelse\np other\nend\np after";
    assert_eq!(out(script), "two\nafter");
}

#[test]
fn repeat_runs_a_fixed_number_of_times() {
    assert_eq!(out("repeat 3\np tick\nend"), "tick\ntick\ntick");
    assert_eq!(out("repeat 0\np tick\nend"), "");
}

#[test]
fn for_walks_a_list_and_a_single_value_alike() {
    assert_eq!(
        out(r#"for x in (list a b c)
p $x
end"#),
        "a\nb\nc"
    );
    // One item is one turn rather than a mistake: an answer that came back as
    // a single thing should still be walkable.
    assert_eq!(out("for x in (ret solo)\np $x\nend"), "solo");
    assert_eq!(out("for x in (nowt)\np $x\nend"), "");
}

#[test]
fn while_runs_until_the_condition_goes_false() {
    assert_eq!(
        out("let n = 0\nwhile ($n < 3)\np $n\nlet n = ($n + 1)\nend"),
        "0\n1\n2"
    );
}

#[test]
fn break_and_continue_do_what_they_say() {
    assert_eq!(
        out("for x in (list 1 2 3 4)\nif ($x == 3)\nbreak\nend\np $x\nend"),
        "1\n2"
    );
    assert_eq!(
        out("for x in (list 1 2 3)\nif ($x == 2)\ncontinue\nend\np $x\nend"),
        "1\n3"
    );
}

#[test]
fn break_outside_a_loop_says_so_rather_than_ending_the_script() {
    assert!(refuse("p one\nbreak").contains("outside a loop"));
}

// ---- functions -----------------------------------------------------------

#[test]
fn a_function_can_be_written_and_called() {
    assert_eq!(
        out(r#"def greet who
p "hello $who"
end
greet Ada"#),
        "hello Ada"
    );
}

#[test]
fn a_function_may_be_called_before_it_is_written() {
    // A script reads top-down. Calling a helper written at the bottom of the
    // file should not be an error.
    assert_eq!(
        out("greet Ada\ndef greet who\np \"hi $who\"\nend"),
        "hi Ada"
    );
}

#[test]
fn a_function_has_its_own_variables() {
    // Sharing the caller's would make a helper's behaviour depend on what the
    // caller happened to name things, which is the fault that makes shell
    // functions miserable.
    let script = "let name = outer\ndef inner\nlet name = changed\nend\ninner\np $name";
    assert_eq!(out(script), "outer");
}

#[test]
fn a_function_cannot_see_what_the_caller_named_things() {
    // Stronger than checking that a helper's changes stay inside it, and it
    // has to be: a *copy* of the caller's variables would pass that check and
    // still be wrong. A helper that can read `$name` because the caller
    // happened to set one works until the day somebody calls it from a script
    // that did not, and then it breaks a long way from the line that changed.
    let complaint = refuse(
        "let secret = outer
def peek
p $secret
end
peek",
    );
    assert!(
        complaint.contains("$secret has not been set"),
        "{complaint}"
    );
}

#[test]
fn a_function_is_given_what_it_was_called_with_and_nothing_else() {
    assert_eq!(
        out("def show a b
p $a $b
end
show one two"),
        "one two"
    );
    // A parameter nobody passed is nothing rather than a complaint, so a
    // helper can take an optional second argument.
    assert_eq!(
        out("def show a b
p \"[$a][$b]\"
end
show one"),
        "[one][]"
    );
}

#[test]
fn a_function_answers_with_return() {
    assert_eq!(
        out("def double n\nreturn ($n * 2)\nend\np (double 21)"),
        "42"
    );
}

#[test]
fn a_function_that_returns_nothing_answers_nothing() {
    assert_eq!(out("def quiet\nreturn\nend\np (quiet)"), "");
}

#[test]
fn a_function_that_calls_itself_forever_is_stopped_and_told_why() {
    let complaint = refuse("def down n\nreturn (down $n)\nend\np (down 3)");
    assert!(
        complaint.contains("calling itself") || complaint.contains("nested more than"),
        "{complaint}"
    );
}

#[test]
fn a_written_command_cannot_shadow_a_real_one() {
    // A script that could take over `p` would quietly change what every other
    // script does.
    assert_eq!(out("def p x\np shadowed\nend\np hello"), "hello");
}

// ---- budgets -------------------------------------------------------------

#[test]
fn a_loop_that_never_ends_is_stopped_and_the_complaint_is_about_the_loop() {
    // Not about steps. The useful thing to say is "your condition never
    // becomes false", which is why the loop ceiling is well under the step one.
    let complaint = refuse("while true\nlet x = 1\nend");
    assert!(complaint.contains("while loop"), "{complaint}");
    assert!(complaint.contains("can become false"), "{complaint}");
}

#[test]
fn printing_more_than_anybody_will_read_is_stopped() {
    let complaint = refuse("let n = 0\nwhile true\np line\nlet n = ($n + 1)\nend");
    assert!(complaint.contains("printed more than"), "{complaint}");
}

#[test]
fn a_repeat_past_the_ceiling_is_refused_before_it_starts() {
    let complaint = refuse("repeat 60000\np x\nend");
    assert!(complaint.contains("turn limit"), "{complaint}");
}

#[test]
fn a_negative_repeat_is_a_mistake_rather_than_nothing() {
    assert!(refuse("repeat -1\np x\nend").contains("zero or more"));
}

// ---- who is asking -------------------------------------------------------

#[test]
fn somebody_only_allowed_to_look_cannot_run_everything() {
    assert_eq!(out_as_guest("p hello"), Ok("hello".to_string()));
    let complaint = out_as_guest("sealed").unwrap_err();
    assert!(complaint.contains("not available"), "{complaint}");
}

#[test]
fn the_bracket_form_is_checked_as_well_as_the_plain_one() {
    // The reason there is one path for running a named thing. Two copies is
    // how the bracket form ends up without the check the statement form has --
    // the exact fault that turns a read-only mode into a suggestion.
    let complaint = out_as_guest("let x = (sealed)").unwrap_err();
    assert!(complaint.contains("not available"), "{complaint}");
}

#[test]
fn a_pipeline_step_is_checked_too() {
    let complaint = out_as_guest("ret x | sealed").unwrap_err();
    assert!(complaint.contains("not available"), "{complaint}");
}

// ---- complaints ----------------------------------------------------------

#[test]
fn a_command_that_does_not_exist_says_so_and_points_somewhere() {
    let complaint = refuse("nonesuch");
    assert!(
        complaint.contains("no command called 'nonesuch'"),
        "{complaint}"
    );
    assert!(complaint.contains("help"), "{complaint}");
}

#[test]
fn a_command_that_fails_is_reported_with_its_line() {
    let complaint = refuse("p one\np two\nboom");
    assert_eq!(complaint, "line 3: deliberate");
}

#[test]
fn a_line_number_is_attached_once_and_not_at_every_frame() {
    // A complaint reading "line 4: line 4: line 4:" helps nobody.
    let complaint = refuse("def inner\nboom\nend\ninner");
    assert_eq!(complaint.matches("line ").count(), 1, "{complaint}");
    assert!(complaint.starts_with("line 2:"), "{complaint}");
}

#[test]
fn a_command_that_needs_arguments_says_how_to_use_it() {
    let complaint = refuse("split");
    assert!(complaint.contains("needs 1 argument"), "{complaint}");
    assert!(complaint.contains("Usage: split"), "{complaint}");
}

// ---- checking without running -------------------------------------------

#[test]
fn a_script_can_be_read_without_being_run() {
    // What a saved script is checked with before it is saved: one that fails
    // on line one next week is worse than one that never saved.
    assert!(ork_critter::check("p hello\nif $a\np yes\nend").is_ok());
    let problem = ork_critter::check("if $a\np yes").unwrap_err();
    assert!(problem.to_string().contains("never closed"), "{problem}");
}

#[test]
fn checking_a_script_runs_none_of_it() {
    // The whole point. A check that printed anything, or stopped a process,
    // would be a check nobody could afford to use.
    let registry = registry();
    let mut collected = Collected::default();
    assert!(ork_critter::check("p hello\nboom").is_ok());
    assert!(collected.lines.is_empty());
    // And the same script does fail when it is actually run.
    assert!(run("p hello\nboom", &registry, &mut collected).is_err());
}

// ---- the registry --------------------------------------------------------

#[test]
fn a_second_command_of_the_same_name_is_refused_unless_it_is_said_out_loud() {
    let mut registry = Registry::new();
    let make = || {
        Box::new(Simple {
            about: Registration::new("p"),
            doing: |_| Ok(None),
        })
    };
    assert!(registry.add(make()).is_ok());
    let complaint = registry.add(make()).unwrap_err();
    assert!(complaint.contains("already exists"), "{complaint}");
    // Deliberate replacement is still possible.
    assert!(registry.replace(make()).is_ok());
}

#[test]
fn a_command_name_must_be_something_somebody_can_type() {
    let mut registry = Registry::new();
    let bad = Box::new(Simple {
        about: Registration::new("Not A Name"),
        doing: |_| Ok(None),
    });
    assert!(registry.add(bad).unwrap_err().contains("lowercase words"));
}

#[test]
fn the_registry_can_list_what_it_holds() {
    // What a reference page is rendered from. FieldKit's `critter-ref.js`
    // reads the live registry rather than a list written beside it, with a
    // check that fails if a registered command is missing from the page -- the
    // same discipline as this repository's rule that a hand-maintained list
    // next to its own assertion cannot fail.
    let registry = registry();
    let listed: Vec<&str> = registry
        .all()
        .iter()
        .map(|about| about.name.as_str())
        .collect();
    assert!(listed.contains(&"p"), "{listed:?}");
    assert!(listed.contains(&"sealed"), "{listed:?}");
    // In a settled order, so a page rendered from it does not shuffle between
    // runs.
    let mut sorted = listed.clone();
    sorted.sort_unstable();
    assert_eq!(listed, sorted);
    // And what it says about a command is what the command says about itself.
    let about = registry.get("split").expect("split is registered").about();
    assert_eq!(about.min_args, 1);
    assert!(about.guest_safe);
}
