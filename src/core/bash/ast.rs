#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstNode {
    Empty,
    Sequence(Vec<AstNode>),
    And(Box<AstNode>, Box<AstNode>),
    Or(Box<AstNode>, Box<AstNode>),
    Pipeline {
        parts: Vec<AstNode>,
        stderr_to_pipe: Vec<bool>,
    },
    Time {
        body: Box<AstNode>,
        posix: bool,
    },
    Coproc {
        name: Option<String>,
        body: Box<AstNode>,
    },
    Negate(Box<AstNode>),
    Background(Box<AstNode>),
    Simple(SimpleCommand),
    ArrayAssign {
        name: String,
        words: Vec<String>,
    },
    If {
        condition: Box<AstNode>,
        then_branch: Box<AstNode>,
        else_branch: Option<Box<AstNode>>,
    },
    For {
        name: String,
        words: Vec<String>,
        body: Box<AstNode>,
    },
    ArithmeticFor {
        init: String,
        condition: String,
        update: String,
        body: Box<AstNode>,
    },
    Select {
        name: String,
        words: Vec<String>,
        body: Box<AstNode>,
    },
    While {
        condition: Box<AstNode>,
        body: Box<AstNode>,
        until: bool,
    },
    Case {
        word: String,
        arms: Vec<CaseArm>,
    },
    Conditional(Vec<String>),
    ArithmeticCommand(String),
    FunctionDef {
        name: String,
        body: Box<AstNode>,
    },
    Group(Box<AstNode>),
    Subshell(Box<AstNode>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseTerminator {
    Break,
    Fallthrough,
    ContinueMatching,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseArm {
    pub patterns: Vec<String>,
    pub body: Box<AstNode>,
    pub terminator: CaseTerminator,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleCommand {
    pub words: Vec<String>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub fd: i32,
    pub kind: RedirectKind,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    Read,
    Write,
    Append,
    DupInput,
    DupOutput,
    HereString,
    ReadWrite,
    Clobber,
    BothWrite,
    BothAppend,
}
