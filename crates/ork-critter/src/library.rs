//! The commands the language comes with.
//!
//! Ported from `registerCore` and `registerMore` in FieldKit's
//! `core/critterscript.js`. Every one of them shapes values and touches
//! nothing else -- no machine, no settings, no files -- which is why all of
//! them are safe for somebody who is only allowed to look.
//!
//! They belong to the language rather than to the tool. `count`, `only` and
//! `field` are what make a pipeline worth writing; without them the language
//! can call verbs and not do anything with their answers, which is a remote
//! control rather than a language.

use crate::run::{Call, Command, Registration, Registry};
use crate::value::{Record, Value};

/// One of the built-in commands.
struct Builtin {
    about: Registration,
    doing: fn(&mut Call<'_, '_>) -> Result<Option<Value>, String>,
}

impl Command for Builtin {
    fn about(&self) -> &Registration {
        &self.about
    }
    fn run(&self, call: &mut Call<'_, '_>) -> Result<Option<Value>, String> {
        (self.doing)(call)
    }
}

/// A registry holding everything the language comes with.
pub fn standard() -> Registry {
    let mut registry = Registry::new();
    add_standard(&mut registry);
    registry
}

/// Add everything the language comes with to a registry that may already hold
/// a host's own commands.
pub fn add_standard(registry: &mut Registry) {
    for (about, doing) in every() {
        registry
            .add(Box::new(Builtin { about, doing }))
            .expect("the built-in names are all different from each other");
    }
}

/// What a value is, said the way somebody would say it.
///
/// Used by the complaints below, so "field needs a record" can say what it got
/// instead. A message naming only what was expected leaves somebody guessing
/// at what they actually passed.
pub fn describe(value: &Value) -> &'static str {
    match value {
        Value::Nothing => "nothing",
        Value::List(_) => "a list",
        Value::Record(_) => "a record",
        Value::Num(_) => "a number",
        Value::Yes(_) => "yes-or-no",
        Value::Text(_) => "text",
    }
}

/// Anything, as a list.
///
/// One value is a list of one. `for x in $answer` should work whether the
/// answer came back as one thing or several, and the same is true of every
/// command here that takes a list.
fn as_list(value: &Value) -> Vec<Value> {
    match value {
        Value::List(items) => items.clone(),
        Value::Nothing => Vec::new(),
        Value::Text(text) if text.is_empty() => Vec::new(),
        single => vec![single.clone()],
    }
}

fn arg(call: &Call<'_, '_>, index: usize) -> Value {
    call.arg(index).cloned().unwrap_or(Value::Nothing)
}

fn text_of(call: &Call<'_, '_>, index: usize) -> String {
    arg(call, index).show()
}

/// Every argument joined by spaces, which is what the text commands take.
fn all_text(call: &Call<'_, '_>) -> String {
    call.words().join(" ")
}

fn number_of(call: &Call<'_, '_>, index: usize, doing: &str) -> Result<f64, String> {
    arg(call, index).numeric(doing)
}

type Entry = (
    Registration,
    fn(&mut Call<'_, '_>) -> Result<Option<Value>, String>,
);

fn shaping(name: &str, usage: &str, min_args: usize, help: &str) -> Registration {
    Registration::new(name)
        .usage(usage)
        .help(help)
        .group("values")
        // Every command in this file shapes a value and reaches nothing.
        .guest_safe(true)
        .min_args(min_args)
}

fn every() -> Vec<Entry> {
    vec![
        // ---- making and measuring ----------------------------------------
        (
            shaping(
                "list",
                "list <items...>",
                0,
                "Make a list from the arguments.",
            ),
            |call| Ok(Some(Value::List(call.args.clone()))),
        ),
        (
            shaping(
                "split",
                "split <text> [separator]",
                1,
                "Split text into a list. The separator is a space unless you say otherwise.",
            ),
            |call| {
                let separator = if call.args.len() > 1 {
                    text_of(call, 1)
                } else {
                    " ".to_string()
                };
                if separator.is_empty() {
                    return Err("split needs something to split on".to_string());
                }
                Ok(Some(Value::List(
                    text_of(call, 0)
                        .split(&separator)
                        // Empty pieces are dropped, so splitting "a  b" on a
                        // space gives two items rather than three.
                        .filter(|piece| !piece.is_empty())
                        .map(|piece| Value::Text(piece.to_string()))
                        .collect(),
                )))
            },
        ),
        (
            shaping(
                "join",
                "join <list> [separator]",
                1,
                "Join a list into text. The separator is a space unless you say otherwise.",
            ),
            |call| {
                let separator = if call.args.len() > 1 {
                    text_of(call, 1)
                } else {
                    " ".to_string()
                };
                let joined = as_list(&arg(call, 0))
                    .iter()
                    .map(Value::show)
                    .collect::<Vec<_>>()
                    .join(&separator);
                Ok(Some(Value::Text(joined)))
            },
        ),
        (
            shaping(
                "count",
                "count <list-or-text>",
                1,
                "How many items in a list, or characters in text.",
            ),
            |call| {
                let how_many = match arg(call, 0) {
                    Value::List(items) => items.len(),
                    other => other.show().chars().count(),
                };
                Ok(Some(Value::Num(how_many as f64)))
            },
        ),
        (
            shaping(
                "item",
                "item <list> <n>",
                2,
                "One item from a list, counting from 1. A negative number counts from the end.",
            ),
            |call| {
                let items = as_list(&arg(call, 0));
                let asked = number_of(call, 1, "item")?;
                // One-based, because "the first item is number zero" is a tax
                // with no benefit at this size, on a language meant for people
                // who have not written code before.
                let index = if asked < 0.0 {
                    items.len() as f64 + asked
                } else {
                    asked - 1.0
                };
                let picked = (index >= 0.0)
                    .then_some(index as usize)
                    .and_then(|index| items.get(index));
                match picked {
                    Some(value) => Ok(Some(value.clone())),
                    None => Err(format!(
                        "there is no item {} -- the list has {}",
                        crate::token::number_shown(asked),
                        items.len()
                    )),
                }
            },
        ),
        (
            shaping(
                "add",
                "add <list> <items...>",
                2,
                "A copy of the list with more items on the end.",
            ),
            |call| {
                // A copy. Changing the original would mean a variable altering
                // itself because it was passed somewhere, which is the sort of
                // thing that is only ever noticed much later.
                let mut items = as_list(&arg(call, 0));
                items.extend(call.args.iter().skip(1).cloned());
                Ok(Some(Value::List(items)))
            },
        ),
        (
            shaping(
                "range",
                "range <n> | range <from> <to>",
                1,
                "A list of numbers, for use with 'for'.",
            ),
            |call| {
                let (from, to) = if call.args.len() > 1 {
                    (number_of(call, 0, "range")?, number_of(call, 1, "range")?)
                } else {
                    (1.0, number_of(call, 0, "range")?)
                };
                let how_many = to - from + 1.0;
                if how_many > crate::run::MAX_LOOP as f64 {
                    return Err(format!(
                        "that range is longer than the {} item limit",
                        crate::run::MAX_LOOP
                    ));
                }
                let mut items = Vec::new();
                let mut at = from;
                while at <= to {
                    items.push(Value::Num(at));
                    at += 1.0;
                }
                Ok(Some(Value::List(items)))
            },
        ),
        // ---- text --------------------------------------------------------
        (shaping("upper", "upper <text>", 1, "Uppercase."), |call| {
            Ok(Some(Value::Text(all_text(call).to_uppercase())))
        }),
        (shaping("lower", "lower <text>", 1, "Lowercase."), |call| {
            Ok(Some(Value::Text(all_text(call).to_lowercase())))
        }),
        (
            shaping("trim", "trim <text>", 1, "Remove the spaces around it."),
            |call| Ok(Some(Value::Text(all_text(call).trim().to_string()))),
        ),
        (
            shaping(
                "replace",
                "replace <text> <find> <with>",
                3,
                "Swap every occurrence. Plain text, not a pattern.",
            ),
            |call| {
                let find = text_of(call, 1);
                if find.is_empty() {
                    return Err("replace needs something to look for".to_string());
                }
                Ok(Some(Value::Text(
                    text_of(call, 0).replace(&find, &text_of(call, 2)),
                )))
            },
        ),
        (
            shaping(
                "starts",
                "starts <text> <prefix>",
                2,
                "Whether the text begins with that.",
            ),
            |call| {
                Ok(Some(Value::Yes(
                    text_of(call, 0).starts_with(&text_of(call, 1)),
                )))
            },
        ),
        (
            shaping(
                "ends",
                "ends <text> <suffix>",
                2,
                "Whether the text finishes with that.",
            ),
            |call| {
                Ok(Some(Value::Yes(
                    text_of(call, 0).ends_with(&text_of(call, 1)),
                )))
            },
        ),
        (
            shaping(
                "lines",
                "lines <text>",
                1,
                "Split text into lines, dropping the blank ones.",
            ),
            |call| {
                Ok(Some(Value::List(
                    text_of(call, 0)
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(|line| Value::Text(line.to_string()))
                        .collect(),
                )))
            },
        ),
        (
            shaping("words", "words <text>", 1, "Split text into words."),
            |call| {
                Ok(Some(Value::List(
                    text_of(call, 0)
                        .split_whitespace()
                        .map(|word| Value::Text(word.to_string()))
                        .collect(),
                )))
            },
        ),
        (
            shaping(
                "has",
                "has <haystack> <needle>",
                2,
                "Whether the text or list contains the second thing.",
            ),
            |call| {
                let haystack = arg(call, 0);
                let needle = arg(call, 1);
                Ok(Some(Value::Yes(match &haystack {
                    Value::List(items) => items.iter().any(|item| item.same_as(&needle)),
                    other => other.show().contains(&needle.show()),
                })))
            },
        ),
        // ---- lists -------------------------------------------------------
        (
            shaping(
                "sort",
                "sort <list> [down]",
                1,
                "Sort a list. Numbers sort as numbers, text as text. Add 'down' to reverse it.",
            ),
            |call| {
                let mut items = as_list(&arg(call, 0));
                let down = call.args.len() > 1
                    && matches!(
                        text_of(call, 1).to_ascii_lowercase().as_str(),
                        "down" | "desc" | "reverse" | "yes" | "true"
                    );
                // Numbers as numbers is the whole point: sorting 2, 10, 9 as
                // text gives 10, 2, 9, which looks like the sort is broken.
                let all_numbers =
                    !items.is_empty() && items.iter().all(|item| item.numeric("sort").is_ok());
                if all_numbers {
                    items.sort_by(|a, b| {
                        let a = a.numeric("sort").unwrap_or_default();
                        let b = b.numeric("sort").unwrap_or_default();
                        a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                    });
                } else {
                    items.sort_by_key(Value::show);
                }
                if down {
                    items.reverse();
                }
                Ok(Some(Value::List(items)))
            },
        ),
        (
            shaping("reverse", "reverse <list-or-text>", 1, "Backwards."),
            |call| {
                Ok(Some(match arg(call, 0) {
                    Value::List(mut items) => {
                        items.reverse();
                        Value::List(items)
                    }
                    other => Value::Text(other.show().chars().rev().collect()),
                }))
            },
        ),
        (
            shaping(
                "unique",
                "unique <list>",
                1,
                "The list with repeats taken out, keeping the first of each.",
            ),
            |call| {
                let mut seen: Vec<String> = Vec::new();
                let mut kept = Vec::new();
                for item in as_list(&arg(call, 0)) {
                    let shown = item.show();
                    if !seen.contains(&shown) {
                        seen.push(shown);
                        kept.push(item);
                    }
                }
                Ok(Some(Value::List(kept)))
            },
        ),
        (
            shaping(
                "first",
                "first <list> [how-many]",
                1,
                "The first item, or the first few as a list.",
            ),
            |call| {
                let items = as_list(&arg(call, 0));
                if call.args.len() < 2 {
                    return Ok(Some(items.first().cloned().unwrap_or(Value::Nothing)));
                }
                let how_many = number_of(call, 1, "first")?.max(0.0) as usize;
                Ok(Some(Value::List(
                    items.into_iter().take(how_many).collect(),
                )))
            },
        ),
        (
            shaping(
                "last",
                "last <list> [how-many]",
                1,
                "The last item, or the last few as a list.",
            ),
            |call| {
                let items = as_list(&arg(call, 0));
                if call.args.len() < 2 {
                    return Ok(Some(items.last().cloned().unwrap_or(Value::Nothing)));
                }
                let how_many = number_of(call, 1, "last")?.max(0.0) as usize;
                let from = items.len().saturating_sub(how_many);
                Ok(Some(Value::List(items[from..].to_vec())))
            },
        ),
        (
            shaping(
                "slice",
                "slice <list> <from> [to]",
                2,
                "Part of a list, counting from 1. 'to' is included. A negative number counts from the end.",
            ),
            |call| {
                let items = as_list(&arg(call, 0));
                let length = items.len() as f64;
                let mut from = number_of(call, 1, "slice")?;
                let mut to = if call.args.len() > 2 {
                    number_of(call, 2, "slice")?
                } else {
                    length
                };
                if from < 0.0 {
                    from += length + 1.0;
                }
                if to < 0.0 {
                    to += length + 1.0;
                }
                let start = (from - 1.0).max(0.0) as usize;
                let end = (to.max(0.0) as usize).min(items.len());
                Ok(Some(Value::List(if start >= end {
                    Vec::new()
                } else {
                    items[start..end].to_vec()
                })))
            },
        ),
        (
            shaping(
                "without",
                "without <list> <items...>",
                2,
                "A copy of the list with those items taken out.",
            ),
            |call| {
                let dropping: Vec<String> = call.args.iter().skip(1).map(Value::show).collect();
                Ok(Some(Value::List(
                    as_list(&arg(call, 0))
                        .into_iter()
                        .filter(|item| !dropping.contains(&item.show()))
                        .collect(),
                )))
            },
        ),
        (
            shaping(
                "only",
                "only <list> <text>",
                2,
                "Just the items that contain that text.",
            ),
            |call| {
                let needle = text_of(call, 1);
                Ok(Some(Value::List(
                    as_list(&arg(call, 0))
                        .into_iter()
                        .filter(|item| item.show().contains(&needle))
                        .collect(),
                )))
            },
        ),
        (
            shaping(
                "where",
                "where <list-of-records> <field> <value>",
                3,
                "Just the records whose field equals that value.",
            ),
            |call| {
                let field = text_of(call, 1);
                let wanted = arg(call, 2);
                Ok(Some(Value::List(
                    as_list(&arg(call, 0))
                        .into_iter()
                        .filter(|item| match item {
                            Value::Record(record) => {
                                record.get(&field).is_some_and(|held| held.same_as(&wanted))
                            }
                            _ => false,
                        })
                        .collect(),
                )))
            },
        ),
        (
            shaping(
                "pluck",
                "pluck <list-of-records> <field>",
                2,
                "One field out of every record, as a list.",
            ),
            |call| {
                let field = text_of(call, 1);
                Ok(Some(Value::List(
                    as_list(&arg(call, 0))
                        .into_iter()
                        .map(|item| match item {
                            Value::Record(record) => {
                                record.get(&field).cloned().unwrap_or(Value::Nothing)
                            }
                            _ => Value::Nothing,
                        })
                        .collect(),
                )))
            },
        ),
        // ---- numbers -----------------------------------------------------
        (
            shaping("sum", "sum <list>", 1, "Add up a list of numbers."),
            |call| {
                let mut total = 0.0;
                for item in as_list(&arg(call, 0)) {
                    total += item.numeric("sum")?;
                }
                Ok(Some(Value::Num(total)))
            },
        ),
        (
            shaping(
                "min",
                "min <list> | min <a> <b> ...",
                1,
                "The smallest number.",
            ),
            |call| extreme(call, "min"),
        ),
        (
            shaping(
                "max",
                "max <list> | max <a> <b> ...",
                1,
                "The largest number.",
            ),
            |call| extreme(call, "max"),
        ),
        (
            shaping("round", "round <number> [places]", 1, "Round a number."),
            |call| {
                let number = number_of(call, 0, "round")?;
                let places = if call.args.len() > 1 {
                    number_of(call, 1, "round")?
                } else {
                    0.0
                };
                let scale = 10f64.powf(places.clamp(0.0, 10.0));
                Ok(Some(Value::Num((number * scale).round() / scale)))
            },
        ),
        (
            shaping(
                "number",
                "number <text>",
                1,
                "Turn text into a number. Says so if it is not one.",
            ),
            |call| Ok(Some(Value::Num(number_of(call, 0, "number")?))),
        ),
        (
            shaping(
                "text",
                "text <anything>",
                1,
                "Turn anything into its printed form.",
            ),
            |call| Ok(Some(Value::Text(text_of(call, 0)))),
        ),
        (
            shaping(
                "kind",
                "kind <anything>",
                1,
                "What sort of value this is: a number, text, a list, a record, yes-or-no.",
            ),
            |call| Ok(Some(Value::Text(describe(&arg(call, 0)).to_string()))),
        ),
        // ---- records -----------------------------------------------------
        (
            shaping(
                "record",
                "record <name> <value> ...",
                2,
                "Build a record from name and value pairs.",
            ),
            |call| {
                if call.args.len() % 2 != 0 {
                    return Err(
                        "record needs pairs -- a name and a value for each field".to_string()
                    );
                }
                let mut record = Record::new();
                for pair in call.args.chunks(2) {
                    record.set(pair[0].show(), pair[1].clone());
                }
                Ok(Some(Value::Record(record)))
            },
        ),
        (
            shaping(
                "field",
                "field <record> <name>",
                2,
                "One field out of a record.",
            ),
            |call| {
                let value = arg(call, 0);
                let Value::Record(record) = &value else {
                    return Err(format!("field needs a record, not {}", describe(&value)));
                };
                let wanted = text_of(call, 1);
                match record.get(&wanted) {
                    Some(found) => Ok(Some(found.clone())),
                    // Naming what IS there. A silent nothing here is how one
                    // mistyped field name becomes an hour of confusion.
                    None => {
                        let names: Vec<&str> = record.iter().map(|(name, _)| name).collect();
                        Err(format!(
                            "that record has no field called '{wanted}'. It has: {}",
                            if names.is_empty() {
                                "nothing".to_string()
                            } else {
                                names.join(", ")
                            }
                        ))
                    }
                }
            },
        ),
        (
            shaping(
                "fields",
                "fields <record>",
                1,
                "The names of a record's fields, as a list.",
            ),
            |call| {
                let value = arg(call, 0);
                let Value::Record(record) = &value else {
                    return Err(format!("fields needs a record, not {}", describe(&value)));
                };
                Ok(Some(Value::List(
                    record
                        .iter()
                        .map(|(name, _)| Value::Text(name.to_string()))
                        .collect(),
                )))
            },
        ),
        (
            shaping(
                "with",
                "with <record> <name> <value>",
                3,
                "A copy of the record with one field set.",
            ),
            |call| {
                let value = arg(call, 0);
                let Value::Record(record) = &value else {
                    return Err(format!("with needs a record, not {}", describe(&value)));
                };
                let mut copy = record.clone();
                copy.set(text_of(call, 1), arg(call, 2));
                Ok(Some(Value::Record(copy)))
            },
        ),
        (
            shaping(
                "json",
                "json <text>",
                1,
                "Read JSON text into records and lists.",
            ),
            |call| {
                let text = text_of(call, 0);
                match serde_json::from_str::<serde_json::Value>(text.trim()) {
                    Ok(parsed) => Ok(Some(from_json(parsed))),
                    Err(problem) => Err(format!("that is not JSON -- {problem}")),
                }
            },
        ),
    ]
}

fn extreme(call: &mut Call<'_, '_>, which: &str) -> Result<Option<Value>, String> {
    let considering = if call.args.len() > 1 {
        call.args.clone()
    } else {
        as_list(&arg(call, 0))
    };
    if considering.is_empty() {
        return Err(format!("{which} needs at least one number"));
    }
    let mut numbers = Vec::with_capacity(considering.len());
    for value in &considering {
        numbers.push(value.numeric(which)?);
    }
    let picked = if which == "min" {
        numbers.iter().copied().fold(f64::INFINITY, f64::min)
    } else {
        numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    };
    Ok(Some(Value::Num(picked)))
}

/// JSON as this language's values.
///
/// A record is exactly the shape JSON reads into, which is deliberate: an
/// answer that came back as JSON is already a record, with nothing to convert
/// and nothing to unwrap.
fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Nothing,
        serde_json::Value::Bool(yes) => Value::Yes(yes),
        serde_json::Value::Number(number) => Value::Num(number.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(text) => Value::Text(text),
        serde_json::Value::Array(items) => Value::List(items.into_iter().map(from_json).collect()),
        serde_json::Value::Object(fields) => {
            let mut record = Record::new();
            for (name, value) in fields {
                record.set(name, from_json(value));
            }
            Value::Record(record)
        }
    }
}
