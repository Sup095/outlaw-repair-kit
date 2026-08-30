//! What a script turned out to say.
//!
//! Ported from FieldKit's `core/critterscript.js`, which builds the same shapes
//! as plain objects. Named types here rather than a general tree, because the
//! interesting property of this language is how few shapes it has, and a type
//! that can only hold those is a type that says so.

/// A value written directly into the source.
///
/// Bare words live here because of rule 1: everything that is not `$name` is a
/// literal, so `say hello` carries the word `hello` and not a lookup of it.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Num(f64),
    Bool(bool),
    Word(String),
}

/// Something that produces a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(Literal),
    /// A quoted string, still holding its `$name`s.
    ///
    /// Interpolation happens when the line runs rather than here, so the text
    /// somebody wrote survives into any complaint about the line it is on.
    Str(String),
    Var(String),
    Bin {
        op: &'static str,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A sign in front of something. The sign is kept rather than folded away,
    /// because `+` in front of a value is not the same as nothing in front of
    /// it once the value is text.
    Neg {
        sign: &'static str,
        of: Box<Expr>,
    },
    Not(Box<Expr>),
    /// `(count $list)` -- run a command and use its answer.
    ///
    /// This one rule is what lets `let`, `for` and `if` use commands at all.
    /// Without it the language has verbs that can only ever print, which is a
    /// toy.
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

/// One step of a pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub name: String,
    pub args: Vec<Expr>,
    /// Whether this step mentions `$it`.
    ///
    /// Worked out while parsing rather than while running, because the answer
    /// decides *where* the piped value goes: a step that mentions `$it` gets it
    /// there, and a step that does not gets it as its first argument.
    pub uses_it: bool,
}

/// One line, or one block, of a script.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        line: usize,
        name: String,
        value: Expr,
    },
    /// `break` and `continue`.
    ///
    /// Signals rather than anything thrown past the budget checks, so neither
    /// can be used to escape a runaway-loop guard.
    Break {
        line: usize,
    },
    Continue {
        line: usize,
    },
    Return {
        line: usize,
        value: Option<Expr>,
    },
    /// A single command with no pipe and no `->`.
    Cmd {
        line: usize,
        name: String,
        args: Vec<Expr>,
    },
    Pipe {
        line: usize,
        stages: Vec<Stage>,
        /// The name after `->`, if there was one.
        into: Option<String>,
    },
    If {
        line: usize,
        cond: Expr,
        body: Vec<Stmt>,
        /// `elif` chains are folded into nested `if`s while parsing, so
        /// running a script needs no third shape for them.
        otherwise: Option<Vec<Stmt>>,
    },
    Repeat {
        line: usize,
        count: Expr,
        body: Vec<Stmt>,
    },
    While {
        line: usize,
        cond: Expr,
        body: Vec<Stmt>,
    },
    For {
        line: usize,
        name: String,
        source: Expr,
        body: Vec<Stmt>,
    },
    Def {
        line: usize,
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
}

impl Stmt {
    /// Which line of the script this came from.
    pub fn line(&self) -> usize {
        match self {
            Stmt::Let { line, .. }
            | Stmt::Break { line }
            | Stmt::Continue { line }
            | Stmt::Return { line, .. }
            | Stmt::Cmd { line, .. }
            | Stmt::Pipe { line, .. }
            | Stmt::If { line, .. }
            | Stmt::Repeat { line, .. }
            | Stmt::While { line, .. }
            | Stmt::For { line, .. }
            | Stmt::Def { line, .. } => *line,
        }
    }
}
