use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Program {
    pub data: BTreeMap<String, Expr>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
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
    OwnDecl { name: String, expr: Expr },
    RefDecl { name: String, target: String },
    Assign { name: String, expr: Expr },
    Instruction { op: Instruction, target: String, rhs: Expr },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While { condition: Expr, body: Vec<Stmt> },
    Repeat { times: Expr, body: Vec<Stmt> },
    CycleContract {
        spec: CycleContract,
        body: Vec<Stmt>,
    },
    PrintBlock(Vec<(String, Expr)>),
    Return(Option<Expr>),
    Expr(Expr),
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
    Number(i64),
    String(String),
    Bool(bool),
    Maybe,
    Var(String),
    Unary { op: UnaryOp, rhs: Box<Expr> },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call { name: String, args: Vec<Expr> },
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
}
