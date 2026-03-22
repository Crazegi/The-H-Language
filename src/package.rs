use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::path::Path;

use crate::bytecode::{BytecodeFunction, BytecodeProgram, Instruction};
use crate::evaluator::Value;

const MAGIC: &[u8; 7] = b"HBCPKG1";

#[derive(Debug, Clone)]
pub struct PackageError {
    pub message: String,
}

impl PackageError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PackageError {}

pub fn write_package(program: &BytecodeProgram, path: &Path) -> Result<(), PackageError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);

    write_u32(&mut out, program.globals.len() as u32);
    for (name, value) in &program.globals {
        write_string(&mut out, name);
        write_value(&mut out, value)?;
    }

    let mut function_names: Vec<&String> = program.functions.keys().collect();
    function_names.sort();
    write_u32(&mut out, function_names.len() as u32);

    for name in function_names {
        let f = &program.functions[name];
        write_string(&mut out, &f.name);

        write_u32(&mut out, f.params.len() as u32);
        for p in &f.params {
            write_string(&mut out, p);
        }

        write_u32(&mut out, f.code.len() as u32);
        for ins in &f.code {
            write_instruction(&mut out, ins)?;
        }
    }

    std::fs::write(path, out)
        .map_err(|e| PackageError::new(format!("Failed to write package: {}", e)))
}

pub fn read_package(path: &Path) -> Result<BytecodeProgram, PackageError> {
    let bytes = std::fs::read(path)
        .map_err(|e| PackageError::new(format!("Failed to read package: {}", e)))?;

    let mut cur = Cursor::new(bytes.as_slice());

    let mut magic = [0u8; 7];
    cur.read_exact(&mut magic)
        .map_err(|_| PackageError::new("Invalid package header"))?;
    if &magic != MAGIC {
        return Err(PackageError::new("Unsupported package format"));
    }

    let global_count = read_u32(&mut cur)? as usize;
    let mut globals = BTreeMap::new();
    for _ in 0..global_count {
        let name = read_string(&mut cur)?;
        let value = read_value(&mut cur)?;
        globals.insert(name, value);
    }

    let function_count = read_u32(&mut cur)? as usize;
    let mut functions = HashMap::new();
    for _ in 0..function_count {
        let name = read_string(&mut cur)?;

        let param_count = read_u32(&mut cur)? as usize;
        let mut params = Vec::with_capacity(param_count);
        for _ in 0..param_count {
            params.push(read_string(&mut cur)?);
        }

        let code_len = read_u32(&mut cur)? as usize;
        let mut code = Vec::with_capacity(code_len);
        for _ in 0..code_len {
            code.push(read_instruction(&mut cur)?);
        }

        functions.insert(
            name.clone(),
            BytecodeFunction {
                name,
                params,
                code,
            },
        );
    }

    Ok(BytecodeProgram { globals, functions })
}

fn write_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_i64(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_usize(out: &mut Vec<u8>, v: usize) {
    write_u32(out, v as u32);
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

fn read_u8(cur: &mut Cursor<&[u8]>) -> Result<u8, PackageError> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)
        .map_err(|_| PackageError::new("Unexpected EOF"))?;
    Ok(b[0])
}

fn read_u32(cur: &mut Cursor<&[u8]>) -> Result<u32, PackageError> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)
        .map_err(|_| PackageError::new("Unexpected EOF"))?;
    Ok(u32::from_le_bytes(b))
}

fn read_i64(cur: &mut Cursor<&[u8]>) -> Result<i64, PackageError> {
    let mut b = [0u8; 8];
    cur.read_exact(&mut b)
        .map_err(|_| PackageError::new("Unexpected EOF"))?;
    Ok(i64::from_le_bytes(b))
}

fn read_usize(cur: &mut Cursor<&[u8]>) -> Result<usize, PackageError> {
    Ok(read_u32(cur)? as usize)
}

fn read_string(cur: &mut Cursor<&[u8]>) -> Result<String, PackageError> {
    let len = read_u32(cur)? as usize;
    let mut b = vec![0u8; len];
    cur.read_exact(&mut b)
        .map_err(|_| PackageError::new("Unexpected EOF while reading string"))?;
    String::from_utf8(b).map_err(|_| PackageError::new("Invalid UTF-8 in package"))
}

fn write_value(out: &mut Vec<u8>, v: &Value) -> Result<(), PackageError> {
    match v {
        Value::Int(n) => {
            write_u8(out, 1);
            write_i64(out, *n);
        }
        Value::Str(s) => {
            write_u8(out, 2);
            write_string(out, s);
        }
        Value::Bool(b) => {
            write_u8(out, 3);
            write_u8(out, if *b { 1 } else { 0 });
        }
        Value::Maybe => {
            write_u8(out, 4);
        }
        Value::Ref(name) => {
            write_u8(out, 5);
            write_string(out, name);
        }
        Value::Unit => {
            write_u8(out, 6);
        }
    }
    Ok(())
}

fn read_value(cur: &mut Cursor<&[u8]>) -> Result<Value, PackageError> {
    match read_u8(cur)? {
        1 => Ok(Value::Int(read_i64(cur)?)),
        2 => Ok(Value::Str(read_string(cur)?)),
        3 => Ok(Value::Bool(read_u8(cur)? != 0)),
        4 => Ok(Value::Maybe),
        5 => Ok(Value::Ref(read_string(cur)?)),
        6 => Ok(Value::Unit),
        _ => Err(PackageError::new("Unknown value tag in package")),
    }
}

fn write_instruction(out: &mut Vec<u8>, ins: &Instruction) -> Result<(), PackageError> {
    match ins {
        Instruction::PushInt(v) => {
            write_u8(out, 1);
            write_i64(out, *v);
        }
        Instruction::PushStr(v) => {
            write_u8(out, 2);
            write_string(out, v);
        }
        Instruction::PushBool(v) => {
            write_u8(out, 3);
            write_u8(out, if *v { 1 } else { 0 });
        }
        Instruction::PushMaybe => write_u8(out, 4),
        Instruction::PushUnit => write_u8(out, 5),
        Instruction::LoadVar(v) => {
            write_u8(out, 6);
            write_string(out, v);
        }
        Instruction::DefineVar(v) => {
            write_u8(out, 7);
            write_string(out, v);
        }
        Instruction::StoreVar(v) => {
            write_u8(out, 8);
            write_string(out, v);
        }
        Instruction::StoreOrDefine(v) => {
            write_u8(out, 9);
            write_string(out, v);
        }
        Instruction::DeclareRef { name, target } => {
            write_u8(out, 10);
            write_string(out, name);
            write_string(out, target);
        }
        Instruction::Add => write_u8(out, 11),
        Instruction::Sub => write_u8(out, 12),
        Instruction::Mul => write_u8(out, 13),
        Instruction::Div => write_u8(out, 14),
        Instruction::Mod => write_u8(out, 15),
        Instruction::Eq => write_u8(out, 16),
        Instruction::Ne => write_u8(out, 17),
        Instruction::Lt => write_u8(out, 18),
        Instruction::Lte => write_u8(out, 19),
        Instruction::Gt => write_u8(out, 20),
        Instruction::Gte => write_u8(out, 21),
        Instruction::And => write_u8(out, 22),
        Instruction::Or => write_u8(out, 23),
        Instruction::Xor => write_u8(out, 24),
        Instruction::Neg => write_u8(out, 25),
        Instruction::Not => write_u8(out, 26),
        Instruction::Cmp3 => write_u8(out, 27),
        Instruction::Jump(v) => {
            write_u8(out, 28);
            write_usize(out, *v);
        }
        Instruction::JumpIfFalse(v) => {
            write_u8(out, 29);
            write_usize(out, *v);
        }
        Instruction::Call(name, argc) => {
            write_u8(out, 30);
            write_string(out, name);
            write_usize(out, *argc);
        }
        Instruction::PrintBegin => write_u8(out, 31),
        Instruction::PrintField(v) => {
            write_u8(out, 32);
            write_string(out, v);
        }
        Instruction::PrintEnd => write_u8(out, 33),
        Instruction::Nop => write_u8(out, 34),
        Instruction::Pop => write_u8(out, 35),
        Instruction::Return => write_u8(out, 36),
    }
    Ok(())
}

fn read_instruction(cur: &mut Cursor<&[u8]>) -> Result<Instruction, PackageError> {
    Ok(match read_u8(cur)? {
        1 => Instruction::PushInt(read_i64(cur)?),
        2 => Instruction::PushStr(read_string(cur)?),
        3 => Instruction::PushBool(read_u8(cur)? != 0),
        4 => Instruction::PushMaybe,
        5 => Instruction::PushUnit,
        6 => Instruction::LoadVar(read_string(cur)?),
        7 => Instruction::DefineVar(read_string(cur)?),
        8 => Instruction::StoreVar(read_string(cur)?),
        9 => Instruction::StoreOrDefine(read_string(cur)?),
        10 => {
            let name = read_string(cur)?;
            let target = read_string(cur)?;
            Instruction::DeclareRef { name, target }
        }
        11 => Instruction::Add,
        12 => Instruction::Sub,
        13 => Instruction::Mul,
        14 => Instruction::Div,
        15 => Instruction::Mod,
        16 => Instruction::Eq,
        17 => Instruction::Ne,
        18 => Instruction::Lt,
        19 => Instruction::Lte,
        20 => Instruction::Gt,
        21 => Instruction::Gte,
        22 => Instruction::And,
        23 => Instruction::Or,
        24 => Instruction::Xor,
        25 => Instruction::Neg,
        26 => Instruction::Not,
        27 => Instruction::Cmp3,
        28 => Instruction::Jump(read_usize(cur)?),
        29 => Instruction::JumpIfFalse(read_usize(cur)?),
        30 => {
            let name = read_string(cur)?;
            let argc = read_usize(cur)?;
            Instruction::Call(name, argc)
        }
        31 => Instruction::PrintBegin,
        32 => Instruction::PrintField(read_string(cur)?),
        33 => Instruction::PrintEnd,
        34 => Instruction::Nop,
        35 => Instruction::Pop,
        36 => Instruction::Return,
        _ => return Err(PackageError::new("Unknown instruction tag in package")),
    })
}
