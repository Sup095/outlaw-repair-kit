//! Turning lines into something that can be run, or into a complaint.
//!
//! A port of the parser in FieldKit's `core/critterscript.js`. Two halves that
//! barely touch: an expression parser doing precedence climbing, and a
//! statement parser that finds blocks by scanning lines for openers and `end`.
//!
//! Blocks are found by scanning rather than by a parser stack, and that is a
//! decision about error messages rather than about parsing: "`if` on line 4 was
//! never closed" is something somebody can act on, and a stack dump is not.

use crate::ast::{Expr, Literal, Stage, Stmt};
use crate::token::{Token, TokenKind, tokenize};

/// How deeply blocks may nest.
///
/// Not a judgement about style. A script is read by people, and past this the
/// thing that fails is the reading rather than the running.
pub const MAX_DEPTH: usize = 24;

/// A script that could not be read, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The line it is on, counting from one. Zero when there is no one line to
    /// blame.
    pub line: usize,
    pub because: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line == 0 {
            out.write_str(&self.because)
        } else {
            write!(out, "line {}: {}", self.line, self.because)
        }
    }
}

impl std::error::Error for Problem {}

fn problem(line: usize, because: impl Into<String>) -> Problem {
    Problem {
        line,
        because: because.into(),
    }
}

type Answer<T> = Result<T, Problem>;

/// How tightly each operator binds.
fn precedence(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        "<" | ">" | "<=" | ">=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => return None,
    })
}

/// `and` and `or` as words, because they read better than the symbols and
/// nobody has to reach for them.
///
/// Normalised here so that everything downstream only ever sees one form.
fn normalise_words(tokens: Vec<Token>) -> Vec<Token> {
    tokens
        .into_iter()
        .map(|token| match token.word() {
            Some("and") => Token {
                kind: TokenKind::Op("&&"),
                glued: false,
            },
            Some("or") => Token {
                kind: TokenKind::Op("||"),
                glued: false,
            },
            _ => token,
        })
        .collect()
}

// ---- expressions ---------------------------------------------------------

struct Parsed {
    node: Expr,
    at: usize,
}

fn parse_expr(tokens: &[Token], mut at: usize, min_prec: u8, line: usize) -> Answer<Parsed> {
    let first = parse_unary(tokens, at, line)?;
    let mut left = first.node;
    at = first.at;

    while at < tokens.len() {
        let Some(op) = tokens[at].op() else { break };
        let Some(prec) = precedence(op) else { break };
        if prec < min_prec {
            break;
        }
        // An operator with nothing to its right is not an operator, it is a
        // literal the caller wants: `join $l -` hands `-` to join as a
        // separator. Without this the loop reaches past the `)` and reports
        // "unexpected )", blaming the bracket for a minus sign three tokens
        // back.
        match tokens.get(at + 1) {
            None => break,
            Some(next) if next.op() == Some(")") => break,
            _ => {}
        }
        let right = parse_expr(tokens, at + 1, prec + 1, line)?;
        left = Expr::Bin {
            op,
            left: Box::new(left),
            right: Box::new(right.node),
        };
        at = right.at;
    }

    Ok(Parsed { node: left, at })
}

fn parse_unary(tokens: &[Token], at: usize, line: usize) -> Answer<Parsed> {
    let Some(token) = tokens.get(at) else {
        return Err(problem(line, "expression ended early"));
    };

    if let Some(sign @ ("-" | "+")) = token.op() {
        let inner = parse_unary(tokens, at + 1, line)?;
        return Ok(Parsed {
            node: Expr::Neg {
                sign,
                of: Box::new(inner.node),
            },
            at: inner.at,
        });
    }

    if token.word() == Some("not") {
        let inner = parse_unary(tokens, at + 1, line)?;
        return Ok(Parsed {
            node: Expr::Not(Box::new(inner.node)),
            at: inner.at,
        });
    }

    if token.op() == Some("(") {
        // Brackets run a command and hand back its answer. A leading word
        // followed by a binary operator is still arithmetic, so
        // `(yes == $answer)` keeps meaning what it looks like; everything else
        // after `(` is a call.
        if let Some(name) = tokens.get(at + 1).and_then(Token::word)
            && !matches!(name, "not" | "true" | "false")
        {
            let operator_follows = tokens
                .get(at + 2)
                .and_then(Token::op)
                .and_then(precedence)
                .is_some();
            if !operator_follows {
                let call = arg_nodes(tokens, at + 2, true, line)?;
                if tokens.get(call.at).and_then(Token::op) != Some(")") {
                    return Err(problem(line, format!("missing closing ) after '{name}'")));
                }
                return Ok(Parsed {
                    node: Expr::Call {
                        name: name.to_string(),
                        args: call.args,
                    },
                    at: call.at + 1,
                });
            }
        }
        let inner = parse_expr(tokens, at + 1, 1, line)?;
        if tokens.get(inner.at).and_then(Token::op) != Some(")") {
            return Err(problem(line, "missing closing )"));
        }
        return Ok(Parsed {
            node: inner.node,
            at: inner.at + 1,
        });
    }

    let node = match &token.kind {
        TokenKind::Num(number) => Expr::Lit(Literal::Num(*number)),
        TokenKind::Str(text) => Expr::Str(text.clone()),
        TokenKind::Var(name) => Expr::Var(name.clone()),
        TokenKind::Word(word) => match word.as_str() {
            "true" => Expr::Lit(Literal::Bool(true)),
            "false" => Expr::Lit(Literal::Bool(false)),
            other => Expr::Lit(Literal::Word(other.to_string())),
        },
        TokenKind::Op(op) => {
            return Err(problem(line, format!("unexpected '{op}' in an expression")));
        }
    };
    Ok(Parsed { node, at: at + 1 })
}

struct Args {
    args: Vec<Expr>,
    at: usize,
}

/// Arguments are one piece each.
///
/// `say hello world` is two arguments, not an expression. A `(` opts into a
/// real expression, which is also how a nested call is written. Shared by the
/// statement parser and the call parser so the two can never disagree about
/// what an argument is.
fn arg_nodes(tokens: &[Token], mut at: usize, stop_at_paren: bool, line: usize) -> Answer<Args> {
    let mut args = Vec::new();
    while at < tokens.len() {
        let token = &tokens[at];
        if stop_at_paren && token.op() == Some(")") {
            break;
        }
        if token.op() == Some("(") {
            let expr = parse_expr(tokens, at, 1, line)?;
            args.push(expr.node);
            at = expr.at;
            continue;
        }
        // A sign glued to its digits is part of the number, so `item $l -1`
        // passes minus one rather than two arguments reading "-" and "1".
        // Before this there was no way to hand a command a negative number at
        // all. Spaced, it stays separate: `f $n - 1` is three arguments, and
        // subtraction in an argument needs brackets, `f ($n - 1)`.
        if token.glued
            && let Some(TokenKind::Num(number)) = tokens.get(at + 1).map(|next| &next.kind)
        {
            let signed = if token.op() == Some("-") {
                -*number
            } else {
                *number
            };
            args.push(Expr::Lit(Literal::Num(signed)));
            at += 2;
            continue;
        }
        args.push(match &token.kind {
            TokenKind::Num(number) => Expr::Lit(Literal::Num(*number)),
            TokenKind::Str(text) => Expr::Str(text.clone()),
            TokenKind::Var(name) => Expr::Var(name.clone()),
            TokenKind::Word(word) => Expr::Lit(Literal::Word(word.clone())),
            TokenKind::Op(op) => Expr::Lit(Literal::Word((*op).to_string())),
        });
        at += 1;
    }
    Ok(Args { args, at })
}

/// Does this step refer to the value coming down the pipe?
///
/// Strings count. `| say "there are $it words"` mentions `$it` inside quotes,
/// where interpolation will find it when the line runs -- and a scan that only
/// looked at variables would decide the step does not use it, prepend the value
/// as an argument, and print it twice. The fault would look like the language
/// repeating itself for no reason.
fn mentions_it(nodes: &[Expr]) -> bool {
    nodes.iter().any(|node| match node {
        Expr::Var(name) => name == "it",
        Expr::Str(text) => text.contains("$it"),
        Expr::Bin { left, right, .. } => {
            mentions_it(std::slice::from_ref(left.as_ref()))
                || mentions_it(std::slice::from_ref(right.as_ref()))
        }
        Expr::Neg { of, .. } => mentions_it(std::slice::from_ref(of.as_ref())),
        Expr::Not(of) => mentions_it(std::slice::from_ref(of.as_ref())),
        Expr::Call { args, .. } => mentions_it(args),
        Expr::Lit(_) => false,
    })
}

/// The value after `let x =`, `for x in`, or `return`.
///
/// A bare word here stays a literal: `let who = Ada` sets `who` to "Ada". An
/// earlier draft of the language made a bare word mean "run this command and
/// use its answer", so that `let n = count $list` would work -- which reads
/// nicely right up until it silently contradicts the one rule the whole thing
/// rests on. Calls in a value need brackets: `let n = (count $list)`. One extra
/// pair, one rule kept.
fn value_expr(tokens: &[Token], from: usize, line: usize, what: &str) -> Answer<Expr> {
    if from >= tokens.len() {
        return Err(problem(line, format!("{what} needs a value")));
    }
    let expr = parse_expr(tokens, from, 1, line)?;
    if expr.at < tokens.len() {
        return Err(problem(
            line,
            format!("trailing '{}' after {what}", tokens[expr.at].shown()),
        ));
    }
    Ok(expr.node)
}

// ---- statements ----------------------------------------------------------

struct Line {
    number: usize,
    tokens: Vec<Token>,
}

/// Words that open a block and are followed by a condition.
fn opens_a_block(word: &str) -> bool {
    matches!(word, "if" | "repeat" | "while")
}

struct Scan {
    lines: Vec<Line>,
    at: usize,
}

struct Closed {
    body: Vec<Stmt>,
    stopped_at: Option<String>,
    line: usize,
}

/// Read a whole script.
pub fn parse(source: &str) -> Answer<Vec<Stmt>> {
    let mut lines = Vec::new();
    for (index, text) in source.split('\n').enumerate() {
        let text = text.strip_suffix('\r').unwrap_or(text);
        let number = index + 1;
        let tokens = tokenize(text).map_err(|unreadable| problem(number, unreadable.because))?;
        lines.push(Line {
            number,
            tokens: normalise_words(tokens),
        });
    }

    let mut scan = Scan { lines, at: 0 };
    let top = block(&mut scan, 0, &[])?;
    if let Some(stray) = top.stopped_at {
        return Err(problem(top.line, format!("stray '{stray}'")));
    }
    Ok(top.body)
}

fn block(scan: &mut Scan, depth: usize, stop_words: &[&str]) -> Answer<Closed> {
    if depth > MAX_DEPTH {
        return Err(problem(
            0,
            format!("blocks nested more than {MAX_DEPTH} deep"),
        ));
    }
    let mut body = Vec::new();

    while scan.at < scan.lines.len() {
        let number = scan.lines[scan.at].number;
        if scan.lines[scan.at].tokens.is_empty() {
            scan.at += 1;
            continue;
        }
        let word = scan.lines[scan.at].tokens[0].word().map(str::to_string);

        if let Some(word) = word.as_deref()
            && stop_words.contains(&word)
        {
            return Ok(Closed {
                body,
                stopped_at: Some(word.to_string()),
                line: number,
            });
        }

        match word.as_deref() {
            // `for name in <list>` is parsed on its own because its header has
            // a shape rather than just a condition.
            Some("for") => {
                let tokens = scan.lines[scan.at].tokens.clone();
                let named_in = tokens
                    .get(2)
                    .is_some_and(|token| token.word() == Some("in"));
                let names_something = tokens.get(1).is_some_and(|token| {
                    matches!(token.kind, TokenKind::Word(_) | TokenKind::Var(_))
                });
                if tokens.len() < 4 || !names_something || !named_in {
                    return Err(problem(number, "for needs   for name in <list>"));
                }
                let name = tokens[1].shown().trim_start_matches('$').to_string();
                let source = value_expr(&tokens, 3, number, "the list")?;
                scan.at += 1;
                let inner = block(scan, depth + 1, &["end"])?;
                if inner.stopped_at.as_deref() != Some("end") {
                    return Err(problem(number, "'for' was never closed with 'end'"));
                }
                scan.at += 1;
                body.push(Stmt::For {
                    line: number,
                    name,
                    source,
                    body: inner.body,
                });
            }

            // Functions were deliberately left out of the language's first
            // version and are the thing that turned out to matter most:
            // without them a script that does one job twice has to say it
            // twice, and composing small pieces is the whole value.
            Some("def") => {
                let tokens = scan.lines[scan.at].tokens.clone();
                let Some(name) = tokens.get(1).and_then(Token::word).map(str::to_string) else {
                    return Err(problem(number, "def needs a name, like   def greet who"));
                };
                if !is_a_function_name(&name) {
                    return Err(problem(
                        number,
                        format!("function names are lowercase words: '{name}'"),
                    ));
                }
                let mut params = Vec::new();
                for token in &tokens[2..] {
                    match &token.kind {
                        TokenKind::Word(word) => params.push(word.clone()),
                        TokenKind::Var(var) => params.push(var.clone()),
                        _ => {
                            return Err(problem(number, "parameter names must be plain words"));
                        }
                    }
                }
                scan.at += 1;
                let inner = block(scan, depth + 1, &["end"])?;
                if inner.stopped_at.as_deref() != Some("end") {
                    return Err(problem(
                        number,
                        format!("'def {name}' was never closed with 'end'"),
                    ));
                }
                scan.at += 1;
                body.push(Stmt::Def {
                    line: number,
                    name,
                    params,
                    body: inner.body,
                });
            }

            Some(opener) if opens_a_block(opener) => {
                let opener = opener.to_string();
                let cond_tokens: Vec<Token> = scan.lines[scan.at].tokens[1..].to_vec();
                if cond_tokens.is_empty() {
                    return Err(problem(
                        number,
                        format!("'{opener}' needs something after it"),
                    ));
                }
                let cond = parse_expr(&cond_tokens, 0, 1, number)?;
                if cond.at < cond_tokens.len() {
                    return Err(problem(
                        number,
                        format!(
                            "trailing '{}' after the {opener} condition",
                            cond_tokens[cond.at].shown()
                        ),
                    ));
                }
                scan.at += 1;

                if opener != "if" {
                    let inner = block(scan, depth + 1, &["end"])?;
                    if inner.stopped_at.as_deref() != Some("end") {
                        return Err(problem(
                            number,
                            format!("'{opener}' was never closed with 'end'"),
                        ));
                    }
                    scan.at += 1;
                    body.push(if opener == "repeat" {
                        Stmt::Repeat {
                            line: number,
                            count: cond.node,
                            body: inner.body,
                        }
                    } else {
                        Stmt::While {
                            line: number,
                            cond: cond.node,
                            body: inner.body,
                        }
                    });
                    continue;
                }

                body.push(if_chain(scan, depth, number, cond.node)?);
            }

            Some(orphan @ ("else" | "end" | "elif")) => {
                return Err(problem(
                    number,
                    format!("'{orphan}' with no matching if/repeat/while/for"),
                ));
            }

            _ => {
                let line = &scan.lines[scan.at];
                let statement = statement(line)?;
                body.push(statement);
                scan.at += 1;
            }
        }
    }

    Ok(Closed {
        body,
        stopped_at: None,
        line: 0,
    })
}

/// An `if` / `elif` / `else` chain, folded into nested `if`s.
///
/// Collected iteratively and folded afterwards, so running a script needs no
/// third shape. It is deliberately **not** parsed by rewriting `elif` into `if`
/// and recursing, which is what the reference implementation did first: the
/// recursive call kept reading past the chain's own `end`, and every following
/// statement in the file landed inside the else branch. The whole rest of the
/// script then ran only when the first condition was false -- silently, with no
/// complaint to point at.
fn if_chain(scan: &mut Scan, depth: usize, opened_at: usize, cond: Expr) -> Answer<Stmt> {
    struct Clause {
        line: usize,
        cond: Expr,
        body: Vec<Stmt>,
    }

    let inner = block(scan, depth + 1, &["end", "else", "elif"])?;
    let mut clauses = vec![Clause {
        line: opened_at,
        cond,
        body: inner.body,
    }];
    let mut stopped = inner.stopped_at;

    while stopped.as_deref() == Some("elif") {
        let number = scan.lines[scan.at].number;
        let cond_tokens: Vec<Token> = scan.lines[scan.at].tokens[1..].to_vec();
        if cond_tokens.is_empty() {
            return Err(problem(number, "'elif' needs a condition"));
        }
        let cond = parse_expr(&cond_tokens, 0, 1, number)?;
        if cond.at < cond_tokens.len() {
            return Err(problem(
                number,
                format!(
                    "trailing '{}' after the elif condition",
                    cond_tokens[cond.at].shown()
                ),
            ));
        }
        scan.at += 1;
        let inner = block(scan, depth + 1, &["end", "else", "elif"])?;
        clauses.push(Clause {
            line: number,
            cond: cond.node,
            body: inner.body,
        });
        stopped = inner.stopped_at;
    }

    let mut otherwise: Option<Vec<Stmt>> = None;
    if stopped.as_deref() == Some("else") {
        scan.at += 1;
        let inner = block(scan, depth + 1, &["end"])?;
        if inner.stopped_at.as_deref() != Some("end") {
            return Err(problem(opened_at, "'else' was never closed with 'end'"));
        }
        otherwise = Some(inner.body);
        stopped = Some("end".to_string());
    }
    if stopped.as_deref() != Some("end") {
        return Err(problem(opened_at, "'if' was never closed with 'end'"));
    }
    scan.at += 1;

    let mut folded = otherwise;
    let mut built = None;
    for clause in clauses.into_iter().rev() {
        let node = Stmt::If {
            line: clause.line,
            cond: clause.cond,
            body: clause.body,
            otherwise: folded,
        };
        folded = Some(vec![node.clone()]);
        built = Some(node);
    }
    Ok(built.expect("a chain always has at least the opening if"))
}

fn is_a_function_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn statement(line: &Line) -> Answer<Stmt> {
    let tokens = &line.tokens;
    let number = line.number;
    let first = tokens[0].word();

    // let name = expression
    if first == Some("let") {
        let names_something = tokens
            .get(1)
            .is_some_and(|token| matches!(token.kind, TokenKind::Word(_) | TokenKind::Var(_)));
        let has_equals = tokens.get(2).is_some_and(|token| token.op() == Some("="));
        if tokens.len() < 4 || !names_something || !has_equals {
            return Err(problem(number, "let needs   let name = value"));
        }
        return Ok(Stmt::Let {
            line: number,
            name: tokens[1].shown().trim_start_matches('$').to_string(),
            value: value_expr(tokens, 3, number, "the value")?,
        });
    }

    if first == Some("break") && tokens.len() == 1 {
        return Ok(Stmt::Break { line: number });
    }
    if first == Some("continue") && tokens.len() == 1 {
        return Ok(Stmt::Continue { line: number });
    }

    if first == Some("return") {
        if tokens.len() == 1 {
            return Ok(Stmt::Return {
                line: number,
                value: None,
            });
        }
        return Ok(Stmt::Return {
            line: number,
            value: Some(value_expr(tokens, 1, number, "the returned value")?),
        });
    }

    if first.is_none() {
        return Err(problem(number, "a line must start with a command name"));
    }

    pipeline(line)
}

/// A line as a sequence of steps.
///
/// **Rules two and three.** `|` sends the answer along; `->` names it. Between
/// them they remove the thing that makes small languages awkward at this size:
/// needing brackets to use one command's answer in another. Before them,
/// counting the words in a file read inside-out, in the order a parser likes:
///
/// ```text
/// let words = (split (read notes) " ")
/// let n = (count $words)
/// ```
///
/// Now it reads in the order the work happens, which is also the order somebody
/// says it out loud:
///
/// ```text
/// read notes | split " " | count -> $n
/// ```
///
/// The answer becomes the **first argument** of the next step -- unless that
/// step mentions `$it`, in which case it goes there and nothing is prepended.
/// One sentence, and it covers both `| count`, where first-argument is
/// obviously right, and `| say "there are $it words"`, where it obviously is
/// not.
fn pipeline(line: &Line) -> Answer<Stmt> {
    let number = line.number;
    let mut tokens: &[Token] = &line.tokens;

    // `-> name` is taken off the end before the split, so a pipe inside the
    // target is impossible rather than merely unlikely.
    let mut into: Option<String> = None;
    for (index, token) in tokens.iter().enumerate() {
        if token.op() != Some("->") {
            continue;
        }
        let target = tokens.get(index + 1);
        let named = target.map(|token| match &token.kind {
            TokenKind::Var(name) => Some(name.clone()),
            TokenKind::Word(word) => Some(word.clone()),
            _ => None,
        });
        let Some(Some(name)) = named else {
            return Err(problem(number, "-> needs a name after it, like  -> $total"));
        };
        if tokens.len() > index + 2 {
            // `shown` already puts the `$` back on a variable, which is what
            // somebody typed and so what they should be shown.
            let shown = target.map(Token::shown).unwrap_or_default();
            return Err(problem(number, format!("nothing may follow  -> {shown}")));
        }
        if index == 0 {
            return Err(problem(number, "-> needs something to name"));
        }
        into = Some(name);
        tokens = &tokens[..index];
        break;
    }

    // Split on pipes that are not inside brackets. A `|` inside `(…)` belongs
    // to the expression there, not to this line.
    let mut stages: Vec<&[Token]> = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.op() {
            Some("(") => depth += 1,
            Some(")") => depth -= 1,
            Some("|") if depth == 0 => {
                stages.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    stages.push(&tokens[start..]);

    let mut built: Vec<Stage> = Vec::with_capacity(stages.len());
    for (index, stage) in stages.iter().enumerate() {
        if stage.is_empty() {
            let where_ = if index == 0 { "before" } else { "after" };
            return Err(problem(number, format!("there is nothing {where_} a |")));
        }
        let Some(name) = stage[0].word() else {
            return Err(problem(
                number,
                "each step of a pipeline starts with a command name",
            ));
        };
        let args = arg_nodes(stage, 1, false, number)?.args;
        let uses_it = mentions_it(&args);
        built.push(Stage {
            name: name.to_string(),
            args,
            uses_it,
        });
    }

    if built.len() == 1 && into.is_none() {
        let only = built.remove(0);
        return Ok(Stmt::Cmd {
            line: number,
            name: only.name,
            args: only.args,
        });
    }
    Ok(Stmt::Pipe {
        line: number,
        stages: built,
        into,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(source: &str) -> Vec<Stmt> {
        parse(source).unwrap_or_else(|problem| panic!("this should read: {problem}"))
    }

    fn refuse(source: &str) -> String {
        parse(source)
            .err()
            .unwrap_or_else(|| panic!("this should not read: {source}"))
            .to_string()
    }

    #[test]
    fn a_plain_line_is_a_command_with_arguments() {
        // Rule one, at the level that matters: `say hello world` is a command
        // and two literals, not an expression anybody has to think about.
        let body = read("say hello world");
        assert_eq!(body.len(), 1);
        let Stmt::Cmd { name, args, line } = &body[0] else {
            panic!("expected a plain command, got {:?}", body[0]);
        };
        assert_eq!(name, "say");
        assert_eq!(*line, 1);
        assert_eq!(
            args,
            &[
                Expr::Lit(Literal::Word("hello".into())),
                Expr::Lit(Literal::Word("world".into()))
            ]
        );
    }

    #[test]
    fn a_word_that_looks_like_a_variable_name_is_still_a_word() {
        let body = read("let name = Ada\nsay name");
        let Stmt::Cmd { args, .. } = &body[1] else {
            panic!("expected a command");
        };
        assert_eq!(args, &[Expr::Lit(Literal::Word("name".into()))]);
    }

    #[test]
    fn a_single_command_is_not_dressed_up_as_a_pipeline() {
        // Worth pinning: one step and no `->` stays the simple shape, so the
        // common case never pays for the machinery of the uncommon one.
        assert!(matches!(read("say hello")[0], Stmt::Cmd { .. }));
        assert!(matches!(read("say hello -> $x")[0], Stmt::Pipe { .. }));
        assert!(matches!(read("say hello | count")[0], Stmt::Pipe { .. }));
    }

    #[test]
    fn a_pipeline_splits_into_steps_in_the_order_written() {
        let body = read("read notes | split \" \" | count -> $n");
        let Stmt::Pipe { stages, into, .. } = &body[0] else {
            panic!("expected a pipeline");
        };
        let names: Vec<&str> = stages.iter().map(|stage| stage.name.as_str()).collect();
        assert_eq!(names, vec!["read", "split", "count"]);
        assert_eq!(into.as_deref(), Some("n"));
    }

    #[test]
    fn a_step_that_mentions_it_is_marked_including_inside_a_string() {
        // The fault this guards against looks like the language repeating
        // itself for no reason: a step mentioning $it only inside quotes would
        // ALSO have the value prepended as an argument, and print it twice.
        let body = read("count | say \"there are $it words\"");
        let Stmt::Pipe { stages, .. } = &body[0] else {
            panic!("expected a pipeline");
        };
        assert!(!stages[0].uses_it, "`count` does not mention it");
        assert!(stages[1].uses_it, "a string mentioning $it does");
    }

    #[test]
    fn a_pipe_inside_brackets_belongs_to_the_expression() {
        // Not `or`: that is normalised to `||` before anything sees it, so
        // it names an operator rather than a command.
        let body = read("say (max $a $b) | count");
        let Stmt::Pipe { stages, .. } = &body[0] else {
            panic!("expected two steps");
        };
        assert_eq!(stages.len(), 2);
    }

    #[test]
    fn an_arrow_takes_a_name_and_nothing_after_it() {
        assert!(refuse("count ->").contains("needs a name after it"));
        assert!(refuse("count -> $n extra").contains("nothing may follow"));
        // A line that opens with `->` never reaches the arrow rule: a line
        // must start with a command name, and that is the more useful thing
        // to be told anyway.
        assert!(refuse("-> $n").contains("must start with a command name"));
    }

    #[test]
    fn an_empty_step_says_so() {
        assert!(refuse("count |").contains("nothing after a |"));
        assert!(refuse("say a | | count").contains("nothing after a |"));
        // And a line that opens with a pipe is turned away one step earlier,
        // by the rule that a line starts with a command name. The parser's
        // "nothing before a |" is therefore unreachable as things stand; it
        // is kept for the same reason the tokenizer keeps its unreachable
        // complaint, which is that the rule in front of it could move.
        assert!(refuse("| count").contains("must start with a command name"));
    }

    #[test]
    fn a_step_starts_with_a_command_name() {
        assert!(refuse("count | $x").contains("starts with a command name"));
    }

    #[test]
    fn precedence_is_the_usual_one() {
        // `(1 + 2 * 3)` must be 1 + (2 * 3). Checked on the shape rather than
        // on an answer, because there is nothing to run this against yet.
        let body = read("say (1 + 2 * 3)");
        let Stmt::Cmd { args, .. } = &body[0] else {
            panic!("expected a command");
        };
        let Expr::Bin { op, right, .. } = &args[0] else {
            panic!("expected an operator at the top, got {:?}", args[0]);
        };
        assert_eq!(*op, "+");
        assert!(
            matches!(right.as_ref(), Expr::Bin { op: "*", .. }),
            "multiplication should have bound tighter: {right:?}"
        );
    }

    #[test]
    fn and_and_or_may_be_written_as_words() {
        let spelled = read("if $a and $b\nsay yes\nend");
        let symbols = read("if $a && $b\nsay yes\nend");
        assert_eq!(spelled, symbols);
    }

    #[test]
    fn a_call_in_a_value_needs_brackets_and_a_bare_word_stays_a_word() {
        // The rule the language rests on, at the one place it is tempting to
        // break: `let who = Ada` sets who to "Ada", and a call needs brackets.
        let body = read("let n = (count $list)");
        let Stmt::Let { value, .. } = &body[0] else {
            panic!("expected a let");
        };
        assert!(matches!(value, Expr::Call { .. }), "{value:?}");

        let body = read("let who = Ada");
        let Stmt::Let { value, .. } = &body[0] else {
            panic!("expected a let");
        };
        assert_eq!(value, &Expr::Lit(Literal::Word("Ada".into())));
    }

    #[test]
    fn arithmetic_in_brackets_is_still_arithmetic() {
        // `(yes == $answer)` must keep meaning what it looks like rather than
        // becoming a call to a command named `yes`.
        let body = read("if (yes == $answer)\nsay ok\nend");
        let Stmt::If { cond, .. } = &body[0] else {
            panic!("expected an if");
        };
        assert!(matches!(cond, Expr::Bin { op: "==", .. }), "{cond:?}");
    }

    #[test]
    fn a_sign_against_its_digits_reaches_the_command_as_one_number() {
        let body = read("item $l -1");
        let Stmt::Cmd { args, .. } = &body[0] else {
            panic!("expected a command");
        };
        assert_eq!(args.len(), 2, "one list and one number: {args:?}");
        assert_eq!(args[1], Expr::Lit(Literal::Num(-1.0)));

        // Spaced, it is three arguments and not subtraction.
        let body = read("down $n - 1");
        let Stmt::Cmd { args, .. } = &body[0] else {
            panic!("expected a command");
        };
        assert_eq!(args.len(), 3, "{args:?}");
    }

    #[test]
    fn an_operator_with_nothing_after_it_is_the_literal_somebody_wanted() {
        // `join $l -` hands `-` to join as a separator. This works because
        // arguments are one piece each and an operator with no expression
        // around it is just a piece -- see `arg_nodes`. It is worth pinning
        // because it is the visible half of the rule; the other half is the
        // complaint below.
        let body = read("say (join $l -)");
        let Stmt::Cmd { args, .. } = &body[0] else {
            panic!("expected a command");
        };
        let Expr::Call { name, args: inner } = &args[0] else {
            panic!("expected a call, got {:?}", args[0]);
        };
        assert_eq!(name, "join");
        assert_eq!(inner.len(), 2, "{inner:?}");
    }

    #[test]
    fn an_if_chain_is_folded_into_nested_ifs() {
        // The fault this shape prevents was silent: parsing `elif` by rewriting
        // it as `if` and recursing read past the chain's own `end`, and every
        // following line landed inside the else branch -- so the rest of the
        // script ran only when the first condition was false, with nothing to
        // point at.
        let body = read("if $a\nsay one\nelif $b\nsay two\nelse\nsay three\nend\nsay after");
        assert_eq!(body.len(), 2, "the chain is one statement: {body:?}");
        assert!(
            matches!(body[1], Stmt::Cmd { .. }),
            "`say after` belongs outside the chain"
        );

        let Stmt::If { otherwise, .. } = &body[0] else {
            panic!("expected an if");
        };
        let inner = otherwise.as_ref().expect("there is an elif");
        assert_eq!(inner.len(), 1);
        let Stmt::If { otherwise, .. } = &inner[0] else {
            panic!("the elif should have become a nested if");
        };
        assert!(otherwise.is_some(), "and the else is inside that");
    }

    #[test]
    fn a_block_that_is_never_closed_names_the_line_it_opened_on() {
        assert_eq!(
            refuse("if $a\nsay one"),
            "line 1: 'if' was never closed with 'end'"
        );
        assert_eq!(
            refuse("say one\nsay two\nwhile $a\nsay three"),
            "line 3: 'while' was never closed with 'end'"
        );
        assert!(refuse("def greet who\nsay hi").contains("'def greet' was never closed"));
        assert!(refuse("for x in $l\nsay $x").contains("'for' was never closed"));
    }

    #[test]
    fn an_end_with_nothing_to_close_says_so() {
        assert!(refuse("say one\nend").contains("no matching"));
        assert!(refuse("else").contains("no matching"));
        assert!(refuse("elif $a").contains("no matching"));
    }

    #[test]
    fn a_header_says_what_shape_it_wanted() {
        assert!(refuse("for x $l\nend").contains("for name in <list>"));
        assert!(refuse("let x 5").contains("let name = value"));
        assert!(refuse("def Greet\nend").contains("lowercase words"));
        assert!(refuse("def\nend").contains("def greet who"));
        assert!(refuse("if\nend").contains("needs something after it"));
    }

    #[test]
    fn a_dangling_operator_blames_itself_rather_than_a_bracket() {
        // The half of the same rule that only shows up in an error message.
        // `parse_expr` stops at an operator with nothing to its right instead
        // of reaching past the `)` -- so the complaint is about the bracket
        // that is genuinely missing, rather than "unexpected )", which blames
        // punctuation three pieces away from the actual mistake.
        assert_eq!(refuse("say ($a -)"), "line 1: missing closing )");
        assert_eq!(refuse("say (1 + )"), "line 1: missing closing )");
        // And an operator that does have something to its right still works.
        assert!(parse("say ($a - $b)").is_ok());
    }

    #[test]
    fn a_complaint_carries_the_line_it_is_about() {
        let problem = parse("say ok\nsay \"unclosed").unwrap_err();
        assert_eq!(problem.line, 2);
        assert!(problem.to_string().starts_with("line 2:"), "{problem}");
    }

    #[test]
    fn blocks_may_not_nest_past_reading() {
        let deep = "if $a\n".repeat(MAX_DEPTH + 2) + "say hi\n" + &"end\n".repeat(MAX_DEPTH + 2);
        assert!(
            parse(&deep)
                .unwrap_err()
                .because
                .contains("nested more than")
        );
    }

    #[test]
    fn a_reasonable_nesting_still_reads() {
        // The other half of the check above: the limit is high enough that
        // nothing anybody would write meets it.
        let fine = "if $a\n".repeat(6) + "say hi\n" + &"end\n".repeat(6);
        assert!(parse(&fine).is_ok());
    }

    #[test]
    fn blank_lines_and_comments_are_not_statements() {
        let body = read("# a note\n\nsay hello\n\n# another\n");
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn windows_line_endings_read_the_same_as_any_other() {
        // This tool is built and run on Windows first. A script written in
        // Notepad must not parse differently from the same script written
        // anywhere else.
        assert_eq!(read("say one\r\nsay two"), read("say one\nsay two"));
    }
}
