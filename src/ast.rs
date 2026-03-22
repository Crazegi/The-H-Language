use std::collections::BTreeMap;

use crate::token::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub data: BTreeMap<String, Expr>,
    pub imports: Vec<String>,
    pub import_spans: Vec<Span>,
    pub functions: Vec<Function>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub span: Span,
    pub is_interrupt: bool,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractPolicy {
    PadNop,
    CompileError,
}

#[derive(Debug, Clone)]
pub struct CycleContract {
    pub cycles: u64,
    pub energy_nj: Option<u64>,
    pub on_underflow: ContractPolicy,
    pub on_overflow: ContractPolicy,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    OwnDecl { name: String, expr: Expr, span: Span },
    RefDecl { name: String, target: String, span: Span },
    PortOwn { port: String, span: Span },
    PortRef { port: String, span: Span },
    YieldPort {
        port: String,
        handler: String,
        body: Vec<Stmt>,
        span: Span,
    },
    Assign { name: String, expr: Expr, span: Span },
    Instruction {
        op: Instruction,
        target: String,
        rhs: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Repeat {
        times: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    For {
        name: String,
        iterable: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    CycleContract {
        spec: CycleContract,
        body: Vec<Stmt>,
        span: Span,
    },
    PrintBlock {
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    Return {
        expr: Option<Expr>,
        span: Span,
    },
    Expr {
        expr: Expr,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    Mov,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Cmp,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64, Span),
    String(String, Span),
    Bool(bool, Span),
    Maybe(Span),
    Var(String, Span),
    Unary {
        op: UnaryOp,
        rhs: Box<Expr>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::OwnDecl { span, .. }
            | Stmt::RefDecl { span, .. }
            | Stmt::PortOwn { span, .. }
            | Stmt::PortRef { span, .. }
            | Stmt::YieldPort { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Instruction { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Repeat { span, .. }
            | Stmt::For { span, .. }
            | Stmt::CycleContract { span, .. }
            | Stmt::PrintBlock { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Expr { span, .. } => *span,
        }
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Number(_, span)
            | Expr::String(_, span)
            | Expr::Bool(_, span)
            | Expr::Maybe(span)
            | Expr::Var(_, span)
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::Call { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Xor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
}
