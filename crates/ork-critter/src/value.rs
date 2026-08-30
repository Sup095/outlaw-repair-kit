//! What a script can hold.
//!
//! Six kinds and no more, ported from FieldKit's `core/critterscript.js`. The
//! rule that keeps the set honest is that **everything can be printed**: a
//! language whose values you cannot show is one you cannot debug from a
//! terminal, and this one is meant to be read back at somebody who is halfway
//! through fixing a machine.

/// A value.
///
/// [`Value::Nothing`] is a real value meaning "nothing", which is not the same
/// as a command having no answer at all -- that is `None`, and the difference
/// matters at a pipe. See [`crate::run`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nothing,
    Num(f64),
    Text(String),
    Yes(bool),
    List(Vec<Value>),
    /// Named fields, in the order they were put there.
    ///
    /// Ordered rather than sorted because a record usually comes from
    /// somewhere that chose an order -- a scan result, a parsed answer -- and
    /// re-sorting it would make the printed form disagree with the thing it
    /// came from.
    Record(Record),
}

/// Named fields, in insertion order.
///
/// A small list rather than a map, because records here are handfuls of fields
/// read by people, and keeping the order they were written in matters more
/// than looking one up quickly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    fields: Vec<(String, Value)>,
}

impl Record {
    pub fn new() -> Self {
        Record::default()
    }

    /// Add a field, or replace one of the same name in place.
    ///
    /// In place, so that setting a field twice does not move it to the end and
    /// quietly reorder what gets printed.
    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        let name = name.into();
        match self.fields.iter_mut().find(|(field, _)| *field == name) {
            Some((_, existing)) => *existing = value,
            None => self.fields.push((name, value)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value)
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.fields
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }
}

impl FromIterator<(String, Value)> for Record {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(fields: T) -> Self {
        let mut record = Record::new();
        for (name, value) in fields {
            record.set(name, value);
        }
        record
    }
}

/// How deep a record is printed before it is elided.
///
/// A record that came from somewhere else can be as deep as that somewhere
/// else felt like making it, and a terminal line is not the place to find that
/// out.
const SHOW_DEPTH: usize = 4;

impl Value {
    /// Whether this counts as a yes.
    ///
    /// The three text spellings are here because the values people type are
    /// words: a setting read back as `off` must not be true simply because it
    /// is a non-empty string.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Nothing => false,
            Value::Yes(yes) => *yes,
            Value::Num(number) => *number != 0.0,
            Value::Text(text) => !matches!(text.as_str(), "" | "false" | "no" | "off"),
            Value::List(items) => !items.is_empty(),
            Value::Record(record) => !record.is_empty(),
        }
    }

    /// This value as text.
    pub fn show(&self) -> String {
        self.shown(0)
    }

    fn shown(&self, depth: usize) -> String {
        match self {
            Value::Nothing => String::new(),
            Value::Yes(yes) => yes.to_string(),
            Value::Num(number) => crate::token::number_shown(*number),
            Value::Text(text) => text.clone(),
            Value::List(items) => items
                .iter()
                .map(|item| item.shown(depth + 1))
                .collect::<Vec<_>>()
                .join(", "),
            Value::Record(record) => {
                if depth > SHOW_DEPTH {
                    return "…".to_string();
                }
                if record.is_empty() {
                    return "(empty record)".to_string();
                }
                record
                    .iter()
                    .map(|(name, value)| {
                        let inner = value.shown(depth + 1);
                        match value {
                            Value::List(_) | Value::Record(_) => format!("{name}=[{inner}]"),
                            _ => format!("{name}={inner}"),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }

    /// This value as a number, or a complaint naming the operation that wanted
    /// one.
    ///
    /// Named after the operation rather than the value, because "'yesterday'
    /// is not a number" leaves somebody looking for where they wrote it, and
    /// "so - cannot use it" tells them.
    pub fn numeric(&self, doing: &str) -> Result<f64, String> {
        match self {
            Value::Num(number) => Ok(*number),
            Value::Yes(yes) => Ok(if *yes { 1.0 } else { 0.0 }),
            Value::Text(text) => text
                .trim()
                .parse::<f64>()
                .map_err(|_| self.not_a_number(doing)),
            _ => Err(self.not_a_number(doing)),
        }
    }

    fn not_a_number(&self, doing: &str) -> String {
        format!(
            "'{}' is not a number, so {doing} cannot use it",
            self.show()
        )
    }

    /// Whether the number this holds, if any, would read as one.
    fn as_number(&self) -> Option<f64> {
        match self {
            Value::Num(number) => Some(*number),
            Value::Text(text) => text.trim().parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Equality as somebody writing a script means it.
    ///
    /// `3` and `"3"` are the same thing here. A language where a number read
    /// from a machine fails to equal the number somebody typed is a language
    /// that makes people write conversions, and conversions are where the
    /// mistakes live.
    pub fn same_as(&self, other: &Value) -> bool {
        if std::mem::discriminant(self) == std::mem::discriminant(other) {
            return self == other;
        }
        if matches!(self, Value::Num(_)) || matches!(other, Value::Num(_)) {
            if let (Some(a), Some(b)) = (self.as_number(), other.as_number()) {
                return a == b;
            }
        }
        self.show() == other.show()
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Self {
        Value::Text(text.to_string())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Self {
        Value::Text(text)
    }
}

impl From<f64> for Value {
    fn from(number: f64) -> Self {
        Value::Num(number)
    }
}

impl From<bool> for Value {
    fn from(yes: bool) -> Self {
        Value::Yes(yes)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Self {
        Value::List(items)
    }
}

impl From<Record> for Value {
    fn from(record: Record) -> Self {
        Value::Record(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: &[(&str, Value)]) -> Record {
        fields
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn everything_can_be_printed() {
        // The rule that keeps the set of kinds honest. If a value could not be
        // shown, a terminal could not be used to find out what a script did.
        let all = [
            Value::Nothing,
            Value::Num(3.0),
            Value::Text("hello".into()),
            Value::Yes(true),
            Value::List(vec![Value::Num(1.0), Value::Text("two".into())]),
            Value::Record(record(&[("name", Value::Text("Ada".into()))])),
        ];
        for value in all {
            // Nothing shows as nothing, which is still a printable answer.
            let _ = value.show();
        }
    }

    #[test]
    fn a_whole_number_prints_without_a_decimal_part() {
        assert_eq!(Value::Num(3.0).show(), "3");
        assert_eq!(Value::Num(3.5).show(), "3.5");
    }

    #[test]
    fn a_list_prints_as_its_items() {
        assert_eq!(
            Value::List(vec![Value::Num(1.0), Value::Num(2.0)]).show(),
            "1, 2"
        );
    }

    #[test]
    fn a_record_prints_its_fields_rather_than_its_type() {
        // The alternative is the thing every language with objects and a
        // terminal gets wrong: a value that shows as its own category.
        let value = Value::Record(record(&[
            ("name", Value::Text("Ada".into())),
            ("age", Value::Num(30.0)),
        ]));
        assert_eq!(value.show(), "name=Ada age=30");
    }

    #[test]
    fn a_record_keeps_the_order_it_was_written_in() {
        let mut record = Record::new();
        record.set("second", Value::Num(2.0));
        record.set("first", Value::Num(1.0));
        assert_eq!(Value::Record(record).show(), "second=2 first=1");
    }

    #[test]
    fn setting_a_field_twice_does_not_move_it() {
        // Otherwise a record that is filled in and then corrected prints in a
        // different order from the one it was written in.
        let mut record = Record::new();
        record.set("a", Value::Num(1.0));
        record.set("b", Value::Num(2.0));
        record.set("a", Value::Num(9.0));
        assert_eq!(Value::Record(record).show(), "a=9 b=2");
    }

    #[test]
    fn a_nested_record_is_shown_rather_than_elided() {
        let inner = record(&[("city", Value::Text("Hull".into()))]);
        let outer = record(&[
            ("name", Value::Text("Ada".into())),
            ("where", Value::Record(inner)),
        ]);
        assert_eq!(Value::Record(outer).show(), "name=Ada where=[city=Hull]");
    }

    #[test]
    fn a_record_deeper_than_a_terminal_line_is_cut_off() {
        // A record fetched from somewhere else can be as deep as that
        // somewhere else felt like making it.
        let mut value = Value::Record(record(&[("leaf", Value::Num(1.0))]));
        for _ in 0..8 {
            value = Value::Record(record(&[("down", value)]));
        }
        assert!(value.show().contains('…'), "{}", value.show());
    }

    #[test]
    fn an_empty_record_says_it_is_empty() {
        // Rather than printing as nothing, which reads as a value that is not
        // there at all.
        assert_eq!(Value::Record(Record::new()).show(), "(empty record)");
    }

    #[test]
    fn the_words_people_type_for_no_are_treated_as_no() {
        // A setting read back as `off` must not be true merely for being a
        // non-empty string.
        for word in ["", "false", "no", "off"] {
            assert!(!Value::Text(word.into()).truthy(), "'{word}' should be no");
        }
        for word in ["true", "yes", "on", "anything"] {
            assert!(Value::Text(word.into()).truthy(), "'{word}' should be yes");
        }
    }

    #[test]
    fn emptiness_is_no_and_having_something_is_yes() {
        assert!(!Value::Nothing.truthy());
        assert!(!Value::Num(0.0).truthy());
        assert!(Value::Num(1.0).truthy());
        assert!(!Value::List(vec![]).truthy());
        assert!(Value::List(vec![Value::Nothing]).truthy());
        assert!(!Value::Record(Record::new()).truthy());
    }

    #[test]
    fn a_number_and_the_same_number_typed_are_equal() {
        // A number read from a machine must equal the number somebody typed,
        // or every script grows a conversion, and conversions are where the
        // mistakes live.
        assert!(Value::Num(3.0).same_as(&Value::Text("3".into())));
        assert!(Value::Text("3".into()).same_as(&Value::Num(3.0)));
        assert!(!Value::Num(3.0).same_as(&Value::Text("three".into())));
    }

    #[test]
    fn things_that_print_the_same_are_equal() {
        assert!(Value::Yes(true).same_as(&Value::Text("true".into())));
        assert!(!Value::Yes(true).same_as(&Value::Text("yes".into())));
    }

    #[test]
    fn asking_a_word_for_a_number_says_what_wanted_one() {
        let complaint = Value::Text("yesterday".into()).numeric("-").unwrap_err();
        assert!(complaint.contains("yesterday"), "{complaint}");
        assert!(complaint.contains('-'), "{complaint}");
    }

    #[test]
    fn a_number_written_as_text_is_still_a_number() {
        assert_eq!(Value::Text(" 42 ".into()).numeric("+"), Ok(42.0));
        assert_eq!(Value::Yes(true).numeric("+"), Ok(1.0));
    }
}
