//! One line of CritterScript, split into pieces.
//!
//! A faithful port of the tokenizer in FieldKit's `core/critterscript.js`. The
//! comments explaining *why* each rule is the way it is are carried across
//! rather than rewritten, because most of them record a fault that was found
//! by using the language rather than by designing it, and a reimplementation
//! that keeps only the code keeps only half of what was learned.
//!
//! Line at a time, and the pieces are tagged so the parser never has to look
//! at characters again.

/// What a piece of a line turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// A quoted string, already unescaped. `$name` inside it is left alone:
    /// interpolation happens when the line runs, so the text somebody wrote
    /// survives into the error message if the line is wrong.
    Str(String),
    /// `$name`, without the `$`.
    Var(String),
    Num(f64),
    /// One of [`OPS`].
    Op(&'static str),
    /// A bare word, which by rule 1 is a literal.
    Word(String),
}

/// A piece of a line, and the one thing spacing tells us.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// A `-` or `+` written against its digits, as in `item $l -1`.
    ///
    /// Whitespace is the only thing that can tell `item $l -1` (minus one, one
    /// argument) from `down $n - 1` (n, minus, one). The tokenizer throws
    /// spacing away, so a sign glued to its digits is marked here and the
    /// argument scanner reads the mark. Expressions ignore it: inside
    /// brackets, `-` is always the operator.
    pub glued: bool,
}

impl Token {
    fn of(kind: TokenKind) -> Self {
        Token { kind, glued: false }
    }

    /// The text of this piece, as it would be named in a complaint.
    pub fn shown(&self) -> String {
        match &self.kind {
            TokenKind::Str(text) => text.clone(),
            TokenKind::Var(name) => format!("${name}"),
            TokenKind::Num(number) => crate::token::number_shown(*number),
            TokenKind::Op(op) => (*op).to_string(),
            TokenKind::Word(word) => word.clone(),
        }
    }

    /// The word this is, if it is one.
    pub fn word(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Word(word) => Some(word),
            _ => None,
        }
    }

    /// The operator this is, if it is one.
    pub fn op(&self) -> Option<&'static str> {
        match &self.kind {
            TokenKind::Op(op) => Some(op),
            _ => None,
        }
    }
}

/// A number the way this language prints one.
///
/// Whole numbers have no decimal part shown, because `count` answering `3.0`
/// would be arithmetic leaking into a sentence somebody reads.
pub(crate) fn number_shown(number: f64) -> String {
    if number.is_finite() && number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        format!("{number}")
    }
}

/// The operators, longest first.
///
/// **The order is the rule.** The scanner takes the first match in this order,
/// so `<=` must not read as `<` then `=`, `->` must not read as `-` then `>`,
/// and `||` must not read as two pipes.
pub const OPS: &[&str] = &[
    "<=", ">=", "==", "!=", "&&", "||", "->", "+", "-", "*", "/", "%", "<", ">", "=", "(", ")",
    ",", "|",
];

/// A line that could not be read, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub because: String,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.because)
    }
}

impl std::error::Error for Unreadable {}

fn unreadable(because: impl Into<String>) -> Unreadable {
    Unreadable {
        because: because.into(),
    }
}

/// Split one line into pieces.
pub fn tokenize(line: &str) -> Result<Vec<Token>, Unreadable> {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];

        if c == ' ' || c == '\t' {
            i += 1;
            continue;
        }
        // A comment runs to the end of the line.
        if c == '#' {
            break;
        }

        // A quoted string. `\"`, `\\`, `\n` and `\t` are understood; `$name`
        // is left in the text and resolved when the line runs.
        if c == '"' || c == '\'' {
            let quote = c;
            let mut buf = String::new();
            let mut closed = false;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    let escaped = chars[i + 1];
                    buf.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                    i += 2;
                    continue;
                }
                if chars[i] == quote {
                    closed = true;
                    i += 1;
                    break;
                }
                buf.push(chars[i]);
                i += 1;
            }
            if !closed {
                return Err(unreadable(format!(
                    "unclosed string -- add a matching {quote}"
                )));
            }
            out.push(Token::of(TokenKind::Str(buf)));
            continue;
        }

        // A variable.
        if c == '$' {
            let mut j = i + 1;
            let mut name = String::new();
            if j < chars.len() && (chars[j].is_ascii_alphabetic() || chars[j] == '_') {
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    name.push(chars[j]);
                    j += 1;
                }
            }
            if name.is_empty() {
                return Err(unreadable("$ must be followed by a name, like $count"));
            }
            out.push(Token::of(TokenKind::Var(name)));
            i = j;
            continue;
        }

        // A number. A leading `-` is the parser's business, so a bare `-5` in
        // an expression still works.
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if c.is_ascii_digit() || (c == '.' && next.is_ascii_digit()) {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len()
                && chars[i] == '.'
                && chars.get(i + 1).is_some_and(char::is_ascii_digit)
            {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let text: String = chars[start..i].iter().collect();
            let number: f64 = text
                .parse()
                .map_err(|_| unreadable(format!("'{text}' is not a number this can read")))?;
            out.push(Token::of(TokenKind::Num(number)));
            continue;
        }

        // An operator. Two-character forms first, so `<=` does not read as `<`
        // followed by `=`.
        let rest: String = chars[i..].iter().collect();
        if let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) {
            let mut token = Token::of(TokenKind::Op(op));
            if (*op == "-" || *op == "+")
                && chars
                    .get(i + 1)
                    .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                token.glued = true;
            }
            out.push(token);
            i += op.chars().count();
            continue;
        }

        // A bare word, which is a literal string by the one rule.
        //
        // `/` and `%` are allowed *inside* a word but not at the start, which
        // is what makes `set relayUrl https://host/health` work without
        // quoting while `(6 / 2)` is still division. The operator branch above
        // runs first, so a slash that begins a piece is division and one glued
        // to a word is part of it.
        //
        // `|` is excluded outright, because it is the pipe: a word that could
        // swallow one would make `count $l|say` mean something different from
        // `count $l | say`, and spacing must not change meaning. `-` is *not*
        // excluded -- command names have hyphens in them, and `->` only
        // becomes an operator when it starts a piece, which is to say after a
        // space.
        let start = i;
        while i < chars.len() && !ends_a_word(chars[i]) {
            i += 1;
        }
        // Unreachable as the scanner stands, and kept anyway. Every character
        // a word cannot contain is one an earlier branch above already claims
        // -- quotes, `$`, `#`, and the operators -- so the only way to arrive
        // here is for somebody to add a character to that list without adding
        // the branch that handles it. This is what that mistake would look
        // like, instead of a silent infinite loop.
        if i == start {
            return Err(unreadable(format!("cannot read character '{c}'")));
        }
        out.push(Token::of(TokenKind::Word(chars[start..i].iter().collect())));
    }

    Ok(out)
}

/// Whether this character cannot be part of a bare word.
fn ends_a_word(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '$' | '#' | '(' | ')' | ',' | '=' | '<' | '>' | '+' | '*' | '|'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<TokenKind> {
        tokenize(line)
            .expect("this line should read")
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    fn word(text: &str) -> TokenKind {
        TokenKind::Word(text.to_string())
    }
    fn var(text: &str) -> TokenKind {
        TokenKind::Var(text.to_string())
    }
    fn text(value: &str) -> TokenKind {
        TokenKind::Str(value.to_string())
    }

    #[test]
    fn a_bare_word_is_a_word() {
        assert_eq!(kinds("say hello"), vec![word("say"), word("hello")]);
    }

    #[test]
    fn a_dollar_name_is_a_variable_and_a_bare_one_is_not() {
        // The whole language rests on this line.
        assert_eq!(kinds("say $name"), vec![word("say"), var("name")]);
        assert_eq!(kinds("say name"), vec![word("say"), word("name")]);
    }

    #[test]
    fn a_dollar_with_no_name_says_what_it_wanted() {
        let complaint = tokenize("say $").unwrap_err();
        assert!(
            complaint.because.contains("$count"),
            "the complaint should show what one looks like, and said: {complaint}"
        );
    }

    #[test]
    fn a_string_keeps_its_dollars_for_later() {
        // Interpolation happens when the line runs. If it happened here, the
        // text somebody wrote would not survive into the error message about
        // the line it was written on.
        assert_eq!(
            kinds(r#"say "there are $n words""#),
            vec![word("say"), text("there are $n words")]
        );
    }

    #[test]
    fn a_string_understands_the_four_escapes() {
        assert_eq!(
            kinds(r#"say "a\tb\nc\"d\\e""#),
            vec![word("say"), text("a\tb\nc\"d\\e")]
        );
    }

    #[test]
    fn an_unclosed_string_says_which_quote_is_missing() {
        let complaint = tokenize("say \"hello").unwrap_err();
        assert!(complaint.because.contains('"'), "{complaint}");
        let complaint = tokenize("say 'hello").unwrap_err();
        assert!(complaint.because.contains('\''), "{complaint}");
    }

    #[test]
    fn a_comment_runs_to_the_end_of_the_line() {
        assert_eq!(
            kinds("say hello # and this is not said"),
            vec![word("say"), word("hello")]
        );
        assert_eq!(kinds("# nothing here"), vec![]);
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        assert_eq!(kinds(r##"say "#1""##), vec![word("say"), text("#1")]);
    }

    #[test]
    fn two_character_operators_are_read_whole() {
        // The order of OPS is the rule. If `<` were tried first, `<=` would
        // read as two pieces and `$a <= $b` would quietly become something
        // else.
        for (line, expected) in [
            ("$a <= $b", "<="),
            ("$a >= $b", ">="),
            ("$a == $b", "=="),
            ("$a != $b", "!="),
            ("$a && $b", "&&"),
            ("$a || $b", "||"),
            ("count -> $n", "->"),
        ] {
            let found = kinds(line);
            assert!(
                found.contains(&TokenKind::Op(expected)),
                "`{line}` did not produce `{expected}`: {found:?}"
            );
        }
    }

    #[test]
    fn a_pipe_is_never_swallowed_by_a_word() {
        // Spacing must not change meaning: `count $l|say` and
        // `count $l | say` are the same line.
        assert_eq!(kinds("count $l|say"), kinds("count $l | say"));
    }

    #[test]
    fn a_url_survives_without_quoting() {
        // The reason `/` and `%` are allowed inside a word but not at the
        // start. Somebody typing a setting should not have to think about it.
        assert_eq!(
            kinds("set relay https://host/health"),
            vec![word("set"), word("relay"), word("https://host/health")]
        );
    }

    #[test]
    fn a_slash_that_starts_a_piece_is_division() {
        assert_eq!(
            kinds("(6 / 2)"),
            vec![
                TokenKind::Op("("),
                TokenKind::Num(6.0),
                TokenKind::Op("/"),
                TokenKind::Num(2.0),
                TokenKind::Op(")")
            ]
        );
    }

    #[test]
    fn a_hyphenated_command_name_is_one_word() {
        // Command names have hyphens in them. `->` only becomes an operator
        // when it starts a piece, which is to say after a space.
        assert_eq!(kinds("set-key cloud"), vec![word("set-key"), word("cloud")]);
    }

    #[test]
    fn a_sign_against_its_digits_is_marked() {
        // The one thing spacing tells us, and the reason the mark exists:
        // before it there was no way to hand a command a negative number.
        let glued = tokenize("item $l -1").unwrap();
        let minus = glued
            .iter()
            .find(|token| token.op() == Some("-"))
            .expect("there is a minus");
        assert!(minus.glued, "a sign written against its digits is glued");

        let spaced = tokenize("down $n - 1").unwrap();
        let minus = spaced
            .iter()
            .find(|token| token.op() == Some("-"))
            .expect("there is a minus");
        assert!(!minus.glued, "a spaced sign is an operator, not a sign");
    }

    #[test]
    fn numbers_read_as_numbers() {
        assert_eq!(kinds("p 42"), vec![word("p"), TokenKind::Num(42.0)]);
        assert_eq!(kinds("p 4.5"), vec![word("p"), TokenKind::Num(4.5)]);
        assert_eq!(kinds("p .5"), vec![word("p"), TokenKind::Num(0.5)]);
    }

    #[test]
    fn a_whole_number_is_shown_without_a_decimal_part() {
        // `count` answering `3.0` would be arithmetic leaking into a sentence.
        assert_eq!(number_shown(3.0), "3");
        assert_eq!(number_shown(-3.0), "-3");
        assert_eq!(number_shown(3.5), "3.5");
    }

    #[test]
    fn a_character_with_no_other_meaning_is_part_of_a_word() {
        // Worth pinning down, because it looks like an oversight and is not.
        // The one rule says everything that is not `$name` is a literal, so a
        // punctuation mark with no job in the language is a character in a
        // word rather than an error. Somebody typing a path, a version, or a
        // pattern should not have to know which marks the language happens to
        // have opinions about.
        assert_eq!(kinds("say ~"), vec![word("say"), word("~")]);
        assert_eq!(kinds("say a~b!c@d"), vec![word("say"), word("a~b!c@d")]);
    }

    #[test]
    fn the_characters_that_do_have_meaning_never_reach_a_word() {
        // The other half of the same rule, and the reason the check above is
        // not simply "anything goes". Every character excluded from a word is
        // excluded because something earlier in the scanner claims it.
        for line in [
            "say (", "say )", "say ,", "say =", "say <", "say >", "say +", "say *",
        ] {
            let found = kinds(line);
            assert!(
                matches!(found[1], TokenKind::Op(_)),
                "`{line}` should end in an operator and gave {found:?}"
            );
        }
    }

    #[test]
    fn an_empty_line_is_no_pieces_rather_than_a_complaint() {
        assert_eq!(kinds(""), vec![]);
        assert_eq!(kinds("   \t  "), vec![]);
    }
}
