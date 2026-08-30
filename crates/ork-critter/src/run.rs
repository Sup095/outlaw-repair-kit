//! Running a script.
//!
//! Ported from the interpreter in FieldKit's `core/critterscript.js`.
//!
//! # Why this is not asynchronous
//!
//! The reference implementation is, because it runs on a page's one thread and
//! everything it can reach is a promise. Here the opposite is true: this crate
//! deliberately depends on nothing, including a runtime, and a future-returning
//! evaluator would put one in the middle of the language. So a command's work
//! is a plain call, and a host with asynchronous work to do -- which this tool
//! has, since a scan takes minutes -- bridges at its own edge rather than
//! making every expression in the language asynchronous to say so.
//!
//! # Budgets, and why they are not an apology
//!
//! A loop with no ceiling is a program that has to be killed from outside. The
//! reference carries budgets because a hung tab on a school Chromebook costs a
//! reload and re-entering a vault password; here it is a repair tool that may
//! be halfway through looking at a broken machine. Either way the trade is the
//! same and it is a good one: a mistake costs a line of feedback rather than
//! the session.

use std::collections::HashMap;

use crate::ast::{Expr, Literal, Stmt};
use crate::value::Value;

/// Total statements run.
pub const MAX_STEPS: usize = 200_000;
/// Lines printed.
pub const MAX_OUTPUT: usize = 2_000;
/// Turns of any single loop.
///
/// Deliberately well under half of [`MAX_STEPS`]: a tight loop body is two
/// statements a turn, so an equal ceiling would mean the step budget fires
/// first and the complaint talks about steps when the useful thing to say is
/// "your loop condition never becomes false".
pub const MAX_LOOP: usize = 50_000;
/// Nested function calls.
pub const MAX_CALL_DEPTH: usize = 60;

/// Something that went wrong while running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The line it happened on, once it is known.
    pub line: Option<usize>,
    pub because: String,
}

impl std::fmt::Display for Fault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(out, "line {line}: {}", self.because),
            None => out.write_str(&self.because),
        }
    }
}

impl std::error::Error for Fault {}

impl From<String> for Fault {
    fn from(because: String) -> Self {
        Fault {
            line: None,
            because,
        }
    }
}

impl From<crate::parse::Problem> for Fault {
    fn from(problem: crate::parse::Problem) -> Self {
        Fault {
            line: (problem.line != 0).then_some(problem.line),
            because: problem.because,
        }
    }
}

fn fault(because: impl Into<String>) -> Fault {
    Fault {
        line: None,
        because: because.into(),
    }
}

type Answer<T> = Result<T, Fault>;

// ---- the registry --------------------------------------------------------

/// What is known about a command, apart from how to run it.
///
/// The same shape the reference implementation carries, including `guest_safe`
/// -- which is the thing this tool needs most from it. See
/// `docs/proposals/critterscript.md`.
#[derive(Debug, Clone)]
pub struct Registration {
    pub name: String,
    pub help: String,
    pub usage: String,
    pub group: String,
    /// Whether somebody who is only allowed to look may run this.
    pub guest_safe: bool,
    pub min_args: usize,
}

impl Registration {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Registration {
            usage: name.clone(),
            name,
            help: String::new(),
            group: "general".to_string(),
            guest_safe: false,
            min_args: 0,
        }
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    pub fn usage(mut self, usage: impl Into<String>) -> Self {
        self.usage = usage.into();
        self
    }

    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }

    pub fn guest_safe(mut self, guest_safe: bool) -> Self {
        self.guest_safe = guest_safe;
        self
    }

    pub fn min_args(mut self, min_args: usize) -> Self {
        self.min_args = min_args;
        self
    }
}

/// What a command is given when it runs.
pub struct Call<'a, 'out> {
    pub args: Vec<Value>,
    speaker: &'a mut Speaker<'out>,
}

impl Call<'_, '_> {
    /// The arguments as text, which is what most commands want.
    pub fn words(&self) -> Vec<String> {
        self.args.iter().map(Value::show).collect()
    }

    /// One argument, or nothing.
    pub fn arg(&self, index: usize) -> Option<&Value> {
        self.args.get(index)
    }

    /// Print a line.
    ///
    /// Fails when the script has printed more than it is allowed to, which is
    /// why it is not simply a write.
    pub fn say(&mut self, text: impl Into<String>) -> Result<(), String> {
        self.speaker.say(text.into()).map_err(|fault| fault.because)
    }
}

/// Something the language can be told to do.
pub trait Command {
    fn about(&self) -> &Registration;

    /// Do it.
    ///
    /// `None` means "no answer", which cannot be piped from. `Some(Nothing)`
    /// means the answer is nothing, which is a real value and passes along.
    fn run(&self, call: &mut Call<'_, '_>) -> Result<Option<Value>, String>;
}

/// Everything the language can be told to do.
#[derive(Default)]
pub struct Registry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// Add a command.
    ///
    /// Refuses a name already in use. Registration is first-come, because a
    /// second module quietly taking a name would change what every existing
    /// script does -- including saved ones -- with nothing on screen to say
    /// so. Deliberate replacement is still possible; it just has to be said
    /// out loud, with [`Registry::replace`].
    pub fn add(&mut self, command: Box<dyn Command>) -> Result<(), String> {
        let name = command.about().name.clone();
        if !is_a_command_name(&name) {
            return Err(format!("command names are lowercase words: '{name}'"));
        }
        if self.commands.contains_key(&name) {
            return Err(format!(
                "a command called '{name}' already exists. Use `replace` if \
                 that is really the intent."
            ));
        }
        self.commands.insert(name, command);
        Ok(())
    }

    /// Add a command, over one of the same name if there is one.
    pub fn replace(&mut self, command: Box<dyn Command>) -> Result<(), String> {
        let name = command.about().name.clone();
        if !is_a_command_name(&name) {
            return Err(format!("command names are lowercase words: '{name}'"));
        }
        self.commands.insert(name, command);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Command> {
        self.commands.get(name).map(AsRef::as_ref)
    }

    /// Every command, by name.
    pub fn all(&self) -> Vec<&Registration> {
        let mut all: Vec<&Registration> = self
            .commands
            .values()
            .map(|command| command.about())
            .collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }
}

fn is_a_command_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ---- output --------------------------------------------------------------

/// Where a script's printing goes.
pub trait Output {
    fn line(&mut self, text: &str);
}

/// Printing that keeps count.
struct Speaker<'a> {
    out: &'a mut dyn Output,
    lines: usize,
}

impl Speaker<'_> {
    fn say(&mut self, text: String) -> Answer<()> {
        self.lines += 1;
        if self.lines > MAX_OUTPUT {
            return Err(fault(format!(
                "this script printed more than {MAX_OUTPUT} lines and was \
                 stopped. Narrow it down, or collect the answer instead."
            )));
        }
        self.out.line(&text);
        Ok(())
    }
}

/// Collects what a script printed, for anything that wants it as text.
#[derive(Debug, Default)]
pub struct Collected {
    pub lines: Vec<String>,
}

impl Output for Collected {
    fn line(&mut self, text: &str) {
        self.lines.push(text.to_string());
    }
}

impl Collected {
    pub fn joined(&self) -> String {
        self.lines.join("\n")
    }
}

// ---- running -------------------------------------------------------------

/// What a block did, apart from run.
///
/// Signals rather than anything unwound past the budget checks. A `break` that
/// escaped a frame is exactly the hole that turns a loop ceiling into a
/// suggestion: every caller here has to look at the answer and decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    CarryOn,
    Break,
    Continue,
    Return,
}

/// One function's variables.
///
/// A function gets its own. Sharing the caller's would make a helper's
/// behaviour depend on what the caller happened to name things, which is the
/// fault that makes shell functions miserable.
#[derive(Debug, Default, Clone)]
struct Vars {
    named: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
struct Defined {
    params: Vec<String>,
    body: Vec<Stmt>,
}

/// What a script left behind.
#[derive(Debug, Default)]
pub struct Finished {
    pub steps: usize,
    pub lines: usize,
    pub returned: Option<Value>,
}

/// A run in progress.
pub struct Run<'a> {
    registry: &'a Registry,
    speaker: Speaker<'a>,
    functions: HashMap<String, Defined>,
    steps: usize,
    depth: usize,
    guest: bool,
    returned: Option<Value>,
}

/// Read a script and run it.
pub fn run(source: &str, registry: &Registry, out: &mut dyn Output) -> Answer<Finished> {
    run_as(source, registry, out, false)
}

/// Read a script and run it as somebody who is only allowed to look.
pub fn run_as_guest(source: &str, registry: &Registry, out: &mut dyn Output) -> Answer<Finished> {
    run_as(source, registry, out, true)
}

fn run_as(
    source: &str,
    registry: &Registry,
    out: &mut dyn Output,
    guest: bool,
) -> Answer<Finished> {
    let body = crate::parse::parse(source)?;
    let mut run = Run {
        registry,
        speaker: Speaker { out, lines: 0 },
        functions: HashMap::new(),
        steps: 0,
        depth: 0,
        guest,
        returned: None,
    };
    let mut vars = Vars::default();
    let signal = run.block(&body, &mut vars)?;
    if matches!(signal, Signal::Break | Signal::Continue) {
        let word = if signal == Signal::Break {
            "break"
        } else {
            "continue"
        };
        return Err(fault(format!("'{word}' used outside a loop")));
    }
    Ok(Finished {
        steps: run.steps,
        lines: run.speaker.lines,
        returned: run.returned,
    })
}

/// Read a script without running it.
///
/// What a saved script is checked with before it is saved, which is worth
/// doing: one that fails on line one next week is worse than one that never
/// saved.
pub fn check(source: &str) -> Answer<()> {
    crate::parse::parse(source).map(|_| ()).map_err(Fault::from)
}

impl Run<'_> {
    fn spend_a_step(&mut self) -> Answer<()> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err(fault(format!(
                "this script ran more than {MAX_STEPS} steps and was stopped. \
                 Check that every loop condition can become false."
            )));
        }
        Ok(())
    }

    fn block(&mut self, body: &[Stmt], vars: &mut Vars) -> Answer<Signal> {
        // Function declarations are taken in first, so a script reads
        // top-down: calling a helper written at the bottom of the file should
        // not be an error.
        for statement in body {
            if let Stmt::Def {
                name, params, body, ..
            } = statement
            {
                self.functions.insert(
                    name.clone(),
                    Defined {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
            }
        }
        for statement in body {
            let signal = self.statement(statement, vars)?;
            if signal != Signal::CarryOn {
                return Ok(signal);
            }
        }
        Ok(Signal::CarryOn)
    }

    fn statement(&mut self, statement: &Stmt, vars: &mut Vars) -> Answer<Signal> {
        self.spend_a_step()?;
        let line = statement.line();
        self.doing(statement, vars).map_err(|problem| {
            // The line is attached once, at the innermost frame that has one.
            // A complaint reading "line 4: line 4: line 4:" helps nobody.
            if problem.line.is_some() {
                problem
            } else {
                Fault {
                    line: Some(line),
                    because: problem.because,
                }
            }
        })
    }

    fn doing(&mut self, statement: &Stmt, vars: &mut Vars) -> Answer<Signal> {
        match statement {
            // Already taken in by `block`.
            Stmt::Def { .. } => Ok(Signal::CarryOn),

            Stmt::Let { name, value, .. } => {
                let value = self.eval(value, vars)?;
                vars.named.insert(name.clone(), value);
                Ok(Signal::CarryOn)
            }

            Stmt::Break { .. } => Ok(Signal::Break),
            Stmt::Continue { .. } => Ok(Signal::Continue),

            Stmt::Return { value, .. } => {
                self.returned = Some(match value {
                    Some(expr) => self.eval(expr, vars)?,
                    None => Value::Nothing,
                });
                Ok(Signal::Return)
            }

            Stmt::If {
                cond,
                body,
                otherwise,
                ..
            } => {
                if self.eval(cond, vars)?.truthy() {
                    self.block(body, vars)
                } else if let Some(otherwise) = otherwise {
                    self.block(otherwise, vars)
                } else {
                    Ok(Signal::CarryOn)
                }
            }

            Stmt::Repeat { count, body, .. } => {
                let times = self.eval(count, vars)?.numeric("repeat")?;
                if times < 0.0 {
                    return Err(fault("repeat needs a count of zero or more"));
                }
                if times > MAX_LOOP as f64 {
                    return Err(fault(format!(
                        "repeat {} is over the {MAX_LOOP} turn limit",
                        crate::token::number_shown(times)
                    )));
                }
                for _ in 0..(times as usize) {
                    match self.block(body, vars)? {
                        Signal::Break => break,
                        Signal::Return => return Ok(Signal::Return),
                        _ => {}
                    }
                }
                Ok(Signal::CarryOn)
            }

            Stmt::For {
                name, source, body, ..
            } => {
                // A single value is one thing to walk over rather than a
                // mistake: `for x in $answer` should work whether the answer
                // came back as one item or several.
                let items = match self.eval(source, vars)? {
                    Value::List(items) => items,
                    Value::Nothing => Vec::new(),
                    Value::Text(text) if text.is_empty() => Vec::new(),
                    single => vec![single],
                };
                if items.len() > MAX_LOOP {
                    return Err(fault(format!(
                        "for over {} items is past the {MAX_LOOP} turn limit",
                        items.len()
                    )));
                }
                for item in items {
                    vars.named.insert(name.clone(), item);
                    match self.block(body, vars)? {
                        Signal::Break => break,
                        Signal::Return => return Ok(Signal::Return),
                        _ => {}
                    }
                }
                Ok(Signal::CarryOn)
            }

            Stmt::While { cond, body, .. } => {
                let mut turns = 0usize;
                while self.eval(cond, vars)?.truthy() {
                    turns += 1;
                    if turns > MAX_LOOP {
                        return Err(fault(format!(
                            "this while loop passed {MAX_LOOP} turns and was \
                             stopped. Check that the condition can become false."
                        )));
                    }
                    match self.block(body, vars)? {
                        Signal::Break => break,
                        Signal::Return => return Ok(Signal::Return),
                        _ => {}
                    }
                }
                Ok(Signal::CarryOn)
            }

            Stmt::Cmd { name, args, .. } => {
                // A statement prints its answer; an expression keeps it. That
                // is the only difference between the two forms.
                let answer = self.invoke(name, args, vars)?;
                self.print_if_there_is_one(answer)?;
                Ok(Signal::CarryOn)
            }

            Stmt::Pipe { stages, into, .. } => {
                let answer = self.pipeline(stages, vars)?;
                match into {
                    Some(name) => {
                        vars.named
                            .insert(name.clone(), answer.unwrap_or(Value::Nothing));
                    }
                    None => self.print_if_there_is_one(answer)?,
                }
                Ok(Signal::CarryOn)
            }
        }
    }

    fn print_if_there_is_one(&mut self, answer: Option<Value>) -> Answer<()> {
        match answer {
            Some(Value::Nothing) | None => Ok(()),
            Some(Value::Text(text)) if text.is_empty() => Ok(()),
            Some(value) => self.speaker.say(value.show()),
        }
    }

    fn pipeline(&mut self, stages: &[crate::ast::Stage], vars: &mut Vars) -> Answer<Option<Value>> {
        // `$it` belongs to the pipeline rather than to the script. Left set
        // afterwards it would read as an ordinary variable on every later
        // line, and a stale one -- the answer of whichever chain happened to
        // run last. Put back rather than removed, so a pipeline written inside
        // a pipeline's argument does not eat the outer one's `$it`.
        let outer_it = vars.named.get("it").cloned();
        let answer = self.through(stages, vars);
        match outer_it {
            Some(value) => {
                vars.named.insert("it".to_string(), value);
            }
            None => {
                vars.named.remove("it");
            }
        }
        answer
    }

    fn through(&mut self, stages: &[crate::ast::Stage], vars: &mut Vars) -> Answer<Option<Value>> {
        let mut carried: Option<Value> = None;
        for (index, stage) in stages.iter().enumerate() {
            let mut args: Vec<Value> = Vec::with_capacity(stage.args.len() + 1);
            if index > 0 {
                let value = carried.clone().unwrap_or(Value::Nothing);
                // Set for every step past the first, whether or not this one
                // uses it, so a later step can still reach back for it inside
                // a string.
                vars.named.insert("it".to_string(), value.clone());
                // Already a value, so it is handed straight in. Turning it
                // back into text would run it through interpolation a second
                // time and eat any `$` the answer happened to contain.
                if !stage.uses_it {
                    args.push(value);
                }
            }
            for arg in &stage.args {
                args.push(self.eval(arg, vars)?);
            }
            let answer = self.call(&stage.name, args, vars)?;

            // A step that answers nothing cannot be piped from, and saying so
            // is far better than what happens otherwise: `say hello | upper`
            // quietly prints "hello" and then uppercases an empty string,
            // which looks like `upper` is broken.
            if answer.is_none() && index < stages.len() - 1 {
                return Err(fault(format!(
                    "'{}' does not have an answer to pass along. Only a step \
                     that produces a value can be followed by |. Try putting \
                     it at the end of the line.",
                    stage.name
                )));
            }
            carried = answer;
        }
        Ok(carried)
    }

    fn eval(&mut self, expr: &Expr, vars: &mut Vars) -> Answer<Value> {
        self.spend_a_step()?;
        match expr {
            Expr::Lit(Literal::Num(number)) => Ok(Value::Num(*number)),
            Expr::Lit(Literal::Bool(yes)) => Ok(Value::Yes(*yes)),
            Expr::Lit(Literal::Word(word)) => Ok(Value::Text(word.clone())),
            Expr::Str(text) => Ok(Value::Text(interpolate(text, vars))),
            Expr::Var(name) => vars.named.get(name).cloned().ok_or_else(|| {
                fault(format!(
                    "${name} has not been set. Use:  let {name} = something"
                ))
            }),
            Expr::Neg { sign, of } => {
                let value = self.eval(of, vars)?;
                let number = value.numeric(sign)?;
                Ok(Value::Num(if *sign == "-" { -number } else { number }))
            }
            Expr::Not(of) => {
                let value = self.eval(of, vars)?;
                Ok(Value::Yes(!value.truthy()))
            }
            Expr::Call { name, args } => {
                let answer = self.invoke(name, args, vars)?;
                Ok(answer.unwrap_or(Value::Nothing))
            }
            Expr::Bin { op, left, right } => self.binary(op, left, right, vars),
        }
    }

    fn binary(
        &mut self,
        op: &'static str,
        left: &Expr,
        right: &Expr,
        vars: &mut Vars,
    ) -> Answer<Value> {
        // Short-circuit before the right side is looked at, so
        // `if $set and (read $set)` does not read something that is not there.
        if op == "&&" {
            let first = self.eval(left, vars)?;
            if !first.truthy() {
                return Ok(Value::Yes(false));
            }
            return Ok(Value::Yes(self.eval(right, vars)?.truthy()));
        }
        if op == "||" {
            let first = self.eval(left, vars)?;
            if first.truthy() {
                return Ok(Value::Yes(true));
            }
            return Ok(Value::Yes(self.eval(right, vars)?.truthy()));
        }

        let a = self.eval(left, vars)?;
        let b = self.eval(right, vars)?;

        match op {
            "==" => return Ok(Value::Yes(a.same_as(&b))),
            "!=" => return Ok(Value::Yes(!a.same_as(&b))),
            "+" => {
                // `+` joins when either side is text that is not a number,
                // which is what somebody writing `say ($a + $b)` almost always
                // means.
                let joining = |value: &Value| matches!(value, Value::Text(text) if text.trim().parse::<f64>().is_err());
                if joining(&a) || joining(&b) {
                    return Ok(Value::Text(format!("{}{}", a.show(), b.show())));
                }
            }
            _ => {}
        }

        let x = a.numeric(op)?;
        let y = b.numeric(op)?;
        Ok(match op {
            "+" => Value::Num(x + y),
            "-" => Value::Num(x - y),
            "*" => Value::Num(x * y),
            "/" => {
                if y == 0.0 {
                    return Err(fault("cannot divide by zero"));
                }
                Value::Num(x / y)
            }
            "%" => {
                if y == 0.0 {
                    return Err(fault("cannot take a remainder by zero"));
                }
                Value::Num(x % y)
            }
            "<" => Value::Yes(x < y),
            ">" => Value::Yes(x > y),
            "<=" => Value::Yes(x <= y),
            ">=" => Value::Yes(x >= y),
            other => return Err(fault(format!("unknown operator {other}"))),
        })
    }

    /// Work out the arguments, then run the named thing.
    fn invoke(&mut self, name: &str, args: &[Expr], vars: &mut Vars) -> Answer<Option<Value>> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg, vars)?);
        }
        self.call(name, values, vars)
    }

    /// One path for "run a named thing and produce an answer", shared by the
    /// plain statement, the pipeline step, and the `( )` call.
    ///
    /// One path on purpose. Two copies of this is how the bracket form ends up
    /// without the guest check the statement form has -- the exact fault that
    /// turns a read-only mode into a suggestion.
    fn call(&mut self, name: &str, args: Vec<Value>, vars: &mut Vars) -> Answer<Option<Value>> {
        // Copied out rather than borrowed through `self`, so that lending
        // the speaker to the command below is not a second borrow of it.
        let registry = self.registry;
        if let Some(command) = registry.get(name) {
            let about = command.about();
            if self.guest && !about.guest_safe {
                return Err(fault(format!(
                    "'{name}' is not available when looking only"
                )));
            }
            if args.len() < about.min_args {
                return Err(fault(format!(
                    "'{name}' needs {} argument(s).  Usage: {}",
                    about.min_args, about.usage
                )));
            }
            // `registry` was copied out of `self` above, so nothing borrows
            // `self` here and the speaker can simply be lent to the command.
            let mut call = Call {
                args,
                speaker: &mut self.speaker,
            };
            return command.run(&mut call).map_err(fault);
        }

        // Written commands win. A script that could shadow one would quietly
        // change what every other script does.
        let Some(defined) = self.functions.get(name).cloned() else {
            return Err(fault(format!("no command called '{name}'. Try:  help")));
        };
        // The caller's variables are deliberately not handed on -- see
        // `call_defined` for why a function gets its own.
        let _ = vars;
        self.call_defined(name, &defined, args).map(Some)
    }

    fn call_defined(&mut self, name: &str, defined: &Defined, args: Vec<Value>) -> Answer<Value> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(fault(format!(
                "functions nested more than {MAX_CALL_DEPTH} deep -- one is \
                 probably calling itself with no way out"
            )));
        }
        let mut inner = Vars::default();
        for (index, param) in defined.params.iter().enumerate() {
            inner.named.insert(
                param.clone(),
                args.get(index).cloned().unwrap_or(Value::Nothing),
            );
        }
        inner
            .named
            .insert("args".to_string(), Value::List(args.clone()));

        self.depth += 1;
        let was_returned = self.returned.take();
        let signal = self.block(&defined.body, &mut inner);
        let returned = self.returned.take();
        self.returned = was_returned;
        self.depth -= 1;

        match signal? {
            Signal::Break => Err(fault(format!(
                "'break' outside a loop, inside function '{name}'"
            ))),
            Signal::Continue => Err(fault(format!(
                "'continue' outside a loop, inside function '{name}'"
            ))),
            _ => Ok(returned.unwrap_or(Value::Nothing)),
        }
    }
}

/// Put `$name` into a string.
///
/// Only inside quotes, only `$name`, and a name that is not set becomes
/// nothing rather than a complaint -- the alternative makes every
/// `"$path/$file"` a landmine.
fn interpolate(text: &str, vars: &Vars) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut name = String::new();
        if j < chars.len() && (chars[j].is_ascii_alphabetic() || chars[j] == '_') {
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                name.push(chars[j]);
                j += 1;
            }
        }
        if name.is_empty() {
            out.push('$');
            i += 1;
            continue;
        }
        if let Some(value) = vars.named.get(&name) {
            out.push_str(&value.show());
        }
        i = j;
    }
    out
}
