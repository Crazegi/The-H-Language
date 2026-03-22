use std::collections::{BTreeMap, HashMap};

use crate::evaluator::Value;

#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    pub globals: BTreeMap<String, Value>,
    pub functions: HashMap<String, BytecodeFunction>,
}

#[derive(Debug, Clone)]
pub struct BytecodeFunction {
    pub name: String,
    pub params: Vec<String>,
    pub code: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub enum Instruction {
    PushInt(i64),
    PushStr(String),
    PushBool(bool),
    PushMaybe,
    PushUnit,
    LoadVar(String),
    DefineVar(String),
    StoreVar(String),
    StoreOrDefine(String),
    DeclareRef { name: String, target: String },
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
    Neg,
    Not,
    Cmp3,
    Jump(usize),
    JumpIfFalse(usize),
    Call(String, usize),
    PrintBegin,
    PrintField(String),
    PrintEnd,
    Nop,
    Pop,
    Return,
}

pub fn disassemble(program: &BytecodeProgram) -> String {
    let mut out = String::new();

    if !program.globals.is_empty() {
        out.push_str("globals:\n");
        for (k, v) in &program.globals {
            out.push_str(&format!("  {} = {}\n", k, v.render()));
        }
    }

    let mut names: Vec<&String> = program.functions.keys().collect();
    names.sort();

    for name in names {
        let f = &program.functions[name];
        out.push_str(&format!("fn {}({}):\n", f.name, f.params.join(", ")));
        for (i, ins) in f.code.iter().enumerate() {
            out.push_str(&format!("  {:04} {:?}\n", i, ins));
        }
    }

    out
}
