//! CritterScript: the language this tool is spoken to in.
//!
//! A port of the reference implementation in FieldKit (`core/critterscript.js`),
//! which is where the language was designed and where it is already used. The
//! plan, what was decided, and what is still open are in
//! `docs/proposals/critterscript.md`.
//!
//! # Three rules
//!
//! 1. `$name` is a variable. Everything else is a literal.
//! 2. `|` sends the answer along.
//! 3. `->` names the answer.
//!
//! Rule 1 is why this fits a repair tool. There is no quoting to memorise, no
//! "when does this expand", no accidental globbing: `scan quick` scans, and a
//! word that looks like a variable name is a word. Somebody reading a script
//! off a forum post can tell what it will do without knowing the language.
//!
//! # This crate depends on nothing else here, and that is the point
//!
//! Not tidiness. A parser able to reach the tool is a parser whose *tests* can
//! reach the tool, and the conformance suite ported from FieldKit has to run
//! against nothing but text -- otherwise a failure in it means either the
//! language is wrong or the machine is, and there is no way to tell which.
//!
//! `ork-core` may depend on this later. This never depends on `ork-core`.

pub mod token;

pub use token::{Token, TokenKind, tokenize};
