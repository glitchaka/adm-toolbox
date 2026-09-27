#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstNode {
    Empty,
    Sequence(Vec<AstNode>),
    And(Box<AstNode>, Box<AstNode>),
    Or(Box<AstNode>, Box<AstNode>),
    Pipeline(Vec<AstNode>),
    Simple(SimpleCommand),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseArm {
    pub patterns: Vec<String>,
    pub body: Box<AstNode>,
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
    Dup,
    HereString,
}
