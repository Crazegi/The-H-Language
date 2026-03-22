use std::collections::{BTreeMap, HashMap};

use crate::ast::{
    BinaryOp, ContractPolicy, Expr, Instruction as AstInstruction, Program, Stmt, UnaryOp,
};
use crate::bytecode::{BytecodeFunction, BytecodeProgram, Instruction};
use crate::evaluator::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleProfile {
    Generic,
    AvrLike,
    CortexM0Like,
}

impl CycleProfile {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "generic" => Some(Self::Generic),
            "avr" | "avr-like" => Some(Self::AvrLike),
            "cortex-m0" | "cortex-m0-like" | "cortexm0" => Some(Self::CortexM0Like),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::AvrLike => "avr-like",
            Self::CortexM0Like => "cortex-m0-like",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub cycle_profile: CycleProfile,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            cycle_profile: CycleProfile::Generic,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContractCompileReport {
    pub function_name: String,
    pub contract_index: usize,
    pub cycle_profile: CycleProfile,
    pub declared_cycles: u64,
    pub measured_cycles: u64,
    pub padded_nops: u64,
    pub final_cycles: u64,
    pub on_underflow: ContractPolicy,
    pub on_overflow: ContractPolicy,
}

#[derive(Debug, Clone)]
pub struct CompileResult {
    pub bytecode: BytecodeProgram,
    pub contract_reports: Vec<ContractCompileReport>,
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
}

impl CompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CompileError {}

pub fn compile_program(program: &Program) -> Result<BytecodeProgram, CompileError> {
    Ok(compile_program_with_options(program, CompileOptions::default())?.bytecode)
}

pub fn compile_program_with_options(
    program: &Program,
    options: CompileOptions,
) -> Result<CompileResult, CompileError> {
    let mut globals = BTreeMap::new();
    for (name, expr) in &program.data {
        let value = const_expr_to_value(expr)?;
        globals.insert(name.clone(), value);
    }

    let mut functions = HashMap::new();
    let mut reports = Vec::new();
    for f in &program.functions {
        let mut ctx = FunctionCompiler::new(f.name.clone(), options.cycle_profile);
        for stmt in &f.body {
            ctx.compile_stmt(stmt)?;
        }

        if !ctx.ends_with_return() {
            ctx.code.push(Instruction::PushUnit);
            ctx.code.push(Instruction::Return);
        }

        functions.insert(
            f.name.clone(),
            BytecodeFunction {
                name: f.name.clone(),
                params: f.params.clone(),
                code: ctx.code,
            },
        );

        reports.extend(ctx.contract_reports);
    }

    Ok(CompileResult {
        bytecode: BytecodeProgram { globals, functions },
        contract_reports: reports,
    })
}

struct FunctionCompiler {
    function_name: String,
    cycle_profile: CycleProfile,
    code: Vec<Instruction>,
    temp_counter: usize,
    contract_counter: usize,
    contract_reports: Vec<ContractCompileReport>,
}

impl FunctionCompiler {
    fn new(function_name: String, cycle_profile: CycleProfile) -> Self {
        Self {
            function_name,
            cycle_profile,
            code: Vec::new(),
            temp_counter: 0,
            contract_counter: 0,
            contract_reports: Vec::new(),
        }
    }
}

impl FunctionCompiler {
    fn ends_with_return(&self) -> bool {
        matches!(self.code.last(), Some(Instruction::Return))
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        match stmt {
            Stmt::OwnDecl { name, expr } => {
                self.compile_expr(expr)?;
                self.code.push(Instruction::DefineVar(name.clone()));
            }
            Stmt::RefDecl { name, target } => {
                self.code.push(Instruction::DeclareRef {
                    name: name.clone(),
                    target: target.clone(),
                });
            }
            Stmt::Assign { name, expr } => {
                self.compile_expr(expr)?;
                self.code.push(Instruction::StoreVar(name.clone()));
            }
            Stmt::Instruction { op, target, rhs } => match op {
                AstInstruction::Mov => {
                    self.compile_expr(rhs)?;
                    if is_memory_target(target) {
                        self.code.push(Instruction::StoreOrDefine(target.clone()));
                    } else {
                        self.code.push(Instruction::StoreVar(target.clone()));
                    }
                }
                AstInstruction::Add => {
                    self.code.push(Instruction::LoadVar(target.clone()));
                    self.compile_expr(rhs)?;
                    self.code.push(Instruction::Add);
                    self.code.push(Instruction::StoreVar(target.clone()));
                }
                AstInstruction::Sub => {
                    self.code.push(Instruction::LoadVar(target.clone()));
                    self.compile_expr(rhs)?;
                    self.code.push(Instruction::Sub);
                    self.code.push(Instruction::StoreVar(target.clone()));
                }
                AstInstruction::Mul => {
                    self.code.push(Instruction::LoadVar(target.clone()));
                    self.compile_expr(rhs)?;
                    self.code.push(Instruction::Mul);
                    self.code.push(Instruction::StoreVar(target.clone()));
                }
                AstInstruction::Div => {
                    self.code.push(Instruction::LoadVar(target.clone()));
                    self.compile_expr(rhs)?;
                    self.code.push(Instruction::Div);
                    self.code.push(Instruction::StoreVar(target.clone()));
                }
                AstInstruction::Mod => {
                    self.code.push(Instruction::LoadVar(target.clone()));
                    self.compile_expr(rhs)?;
                    self.code.push(Instruction::Mod);
                    self.code.push(Instruction::StoreVar(target.clone()));
                }
                AstInstruction::Cmp => {
                    self.code.push(Instruction::LoadVar(target.clone()));
                    self.compile_expr(rhs)?;
                    self.code.push(Instruction::Cmp3);
                    self.code.push(Instruction::StoreOrDefine("cmp".to_string()));
                }
            },
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                self.compile_expr(condition)?;
                let jump_if_false_pos = self.code.len();
                self.code.push(Instruction::JumpIfFalse(usize::MAX));

                for s in then_body {
                    self.compile_stmt(s)?;
                }

                if else_body.is_empty() {
                    let end_pos = self.code.len();
                    self.patch_jump(jump_if_false_pos, end_pos)?;
                } else {
                    let jump_end_pos = self.code.len();
                    self.code.push(Instruction::Jump(usize::MAX));
                    let else_start = self.code.len();
                    self.patch_jump(jump_if_false_pos, else_start)?;

                    for s in else_body {
                        self.compile_stmt(s)?;
                    }
                    let end_pos = self.code.len();
                    self.patch_jump(jump_end_pos, end_pos)?;
                }
            }
            Stmt::While { condition, body } => {
                let loop_start = self.code.len();
                self.compile_expr(condition)?;
                let jump_if_false_pos = self.code.len();
                self.code.push(Instruction::JumpIfFalse(usize::MAX));

                for s in body {
                    self.compile_stmt(s)?;
                }

                self.code.push(Instruction::Jump(loop_start));
                let loop_end = self.code.len();
                self.patch_jump(jump_if_false_pos, loop_end)?;
            }
            Stmt::Repeat { times, body } => {
                self.compile_expr(times)?;
                let counter_name = self.next_temp("repeat_counter");
                self.code.push(Instruction::DefineVar(counter_name.clone()));

                let loop_start = self.code.len();
                self.code.push(Instruction::LoadVar(counter_name.clone()));
                self.code.push(Instruction::PushInt(0));
                self.code.push(Instruction::Gt);
                let jump_if_false_pos = self.code.len();
                self.code.push(Instruction::JumpIfFalse(usize::MAX));

                for s in body {
                    self.compile_stmt(s)?;
                }

                self.code.push(Instruction::LoadVar(counter_name.clone()));
                self.code.push(Instruction::PushInt(1));
                self.code.push(Instruction::Sub);
                self.code.push(Instruction::StoreVar(counter_name));
                self.code.push(Instruction::Jump(loop_start));

                let loop_end = self.code.len();
                self.patch_jump(jump_if_false_pos, loop_end)?;
            }
            Stmt::CycleContract { .. } => {
                self.compile_cycle_contract(stmt)?;
            }
            Stmt::PrintBlock(fields) => {
                self.code.push(Instruction::PrintBegin);
                for (key, expr) in fields {
                    self.compile_expr(expr)?;
                    self.code.push(Instruction::PrintField(key.clone()));
                }
                self.code.push(Instruction::PrintEnd);
            }
            Stmt::Return(expr) => {
                match expr {
                    Some(e) => self.compile_expr(e)?,
                    None => self.code.push(Instruction::PushUnit),
                }
                self.code.push(Instruction::Return);
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?;
                self.code.push(Instruction::Pop);
            }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Number(v) => self.code.push(Instruction::PushInt(*v)),
            Expr::String(v) => self.code.push(Instruction::PushStr(v.clone())),
            Expr::Bool(v) => self.code.push(Instruction::PushBool(*v)),
            Expr::Maybe => self.code.push(Instruction::PushMaybe),
            Expr::Var(v) => self.code.push(Instruction::LoadVar(v.clone())),
            Expr::Unary { op, rhs } => {
                self.compile_expr(rhs)?;
                match op {
                    UnaryOp::Neg => self.code.push(Instruction::Neg),
                    UnaryOp::Not => self.code.push(Instruction::Not),
                }
            }
            Expr::Binary { left, op, right } => {
                self.compile_expr(left)?;
                self.compile_expr(right)?;
                self.code.push(match op {
                    BinaryOp::Add => Instruction::Add,
                    BinaryOp::Sub => Instruction::Sub,
                    BinaryOp::Mul => Instruction::Mul,
                    BinaryOp::Div => Instruction::Div,
                    BinaryOp::Mod => Instruction::Mod,
                    BinaryOp::Eq => Instruction::Eq,
                    BinaryOp::Ne => Instruction::Ne,
                    BinaryOp::Lt => Instruction::Lt,
                    BinaryOp::Lte => Instruction::Lte,
                    BinaryOp::Gt => Instruction::Gt,
                    BinaryOp::Gte => Instruction::Gte,
                    BinaryOp::And => Instruction::And,
                    BinaryOp::Or => Instruction::Or,
                    BinaryOp::Xor => Instruction::Xor,
                });
            }
            Expr::Call { name, args } => {
                for arg in args {
                    self.compile_expr(arg)?;
                }
                self.code.push(Instruction::Call(name.clone(), args.len()));
            }
        }
        Ok(())
    }

    fn patch_jump(&mut self, pos: usize, target: usize) -> Result<(), CompileError> {
        let Some(ins) = self.code.get_mut(pos) else {
            return Err(CompileError::new("Internal compiler error: bad jump index"));
        };

        match ins {
            Instruction::Jump(addr) | Instruction::JumpIfFalse(addr) => {
                *addr = target;
                Ok(())
            }
            _ => Err(CompileError::new(
                "Internal compiler error: expected jump instruction",
            )),
        }
    }

    fn next_temp(&mut self, prefix: &str) -> String {
        let name = format!("__{}_{}", prefix, self.temp_counter);
        self.temp_counter += 1;
        name
    }

    fn compile_cycle_contract(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        let Stmt::CycleContract { spec, body } = stmt else {
            return Err(CompileError::new(
                "Internal compiler error: expected cycle contract statement",
            ));
        };

        self.contract_counter += 1;
        let contract_index = self.contract_counter;

        let mut total_cycles: u64 = 0;
        for s in body {
            total_cycles = total_cycles
                .checked_add(stmt_cycle_cost(s, self.cycle_profile)?)
                .ok_or_else(|| CompileError::new("Cycle count overflow in contract block"))?;
            self.compile_stmt(s)?;
        }

        let mut padded_nops = 0u64;

        if total_cycles > spec.cycles {
            match spec.on_overflow {
                ContractPolicy::CompileError => {
                    return Err(CompileError::new(format!(
                        "Cycle contract overflow in function `{}` contract #{} (profile: {}): block costs {} cycles but contract allows {}",
                        self.function_name,
                        contract_index,
                        self.cycle_profile.as_str(),
                        total_cycles,
                        spec.cycles
                    )));
                }
                ContractPolicy::PadNop => {
                    // Overflow cannot be solved by padding; keep generated code unchanged.
                }
            }
        } else if total_cycles < spec.cycles {
            let deficit = spec.cycles - total_cycles;
            match spec.on_underflow {
                ContractPolicy::CompileError => {
                    return Err(CompileError::new(format!(
                        "Cycle contract underflow in function `{}` contract #{} (profile: {}): block costs {} cycles but contract requires {}",
                        self.function_name,
                        contract_index,
                        self.cycle_profile.as_str(),
                        total_cycles,
                        spec.cycles
                    )));
                }
                ContractPolicy::PadNop => {
                    padded_nops = deficit;
                    for _ in 0..deficit {
                        self.code.push(Instruction::Nop);
                    }
                }
            }
        }

        let final_cycles = total_cycles + padded_nops;
        self.contract_reports.push(ContractCompileReport {
            function_name: self.function_name.clone(),
            contract_index,
            cycle_profile: self.cycle_profile,
            declared_cycles: spec.cycles,
            measured_cycles: total_cycles,
            padded_nops,
            final_cycles,
            on_underflow: spec.on_underflow,
            on_overflow: spec.on_overflow,
        });

        Ok(())
    }
}

fn const_expr_to_value(expr: &Expr) -> Result<Value, CompileError> {
    match expr {
        Expr::Number(v) => Ok(Value::Int(*v)),
        Expr::String(v) => Ok(Value::Str(v.clone())),
        Expr::Bool(v) => Ok(Value::Bool(*v)),
        Expr::Maybe => Ok(Value::Maybe),
        _ => Err(CompileError::new(
            "Data section supports only literal constants in compiled mode",
        )),
    }
}

fn is_memory_target(target: &str) -> bool {
    target.starts_with('[') && target.ends_with(']') && target.len() > 2
}

fn stmt_cycle_cost(stmt: &Stmt, profile: CycleProfile) -> Result<u64, CompileError> {
    match stmt {
        Stmt::Instruction { op, .. } => Ok(match op {
            AstInstruction::Mov => 1,
            AstInstruction::Add | AstInstruction::Sub => 1,
            AstInstruction::Cmp => 1,
            AstInstruction::Mul => match profile {
                CycleProfile::Generic => 2,
                CycleProfile::AvrLike => 3,
                CycleProfile::CortexM0Like => 1,
            },
            AstInstruction::Div => match profile {
                CycleProfile::Generic => 2,
                CycleProfile::AvrLike => 4,
                CycleProfile::CortexM0Like => 8,
            },
            AstInstruction::Mod => match profile {
                CycleProfile::Generic => 2,
                CycleProfile::AvrLike => 4,
                CycleProfile::CortexM0Like => 8,
            },
        }),
        _ => Err(CompileError::new(
            "Cycle contract execute block supports only assembly instructions",
        )),
    }
}

pub fn render_contract_report_text(reports: &[ContractCompileReport]) -> String {
    if reports.is_empty() {
        return "no_cycle_contracts".to_string();
    }

    let mut out = String::new();
    for report in reports {
        out.push_str(&format!(
            "function={} contract=#{} profile={} declared={} measured={} padded_nops={} final={} underflow={:?} overflow={:?}\n",
            report.function_name,
            report.contract_index,
            report.cycle_profile.as_str(),
            report.declared_cycles,
            report.measured_cycles,
            report.padded_nops,
            report.final_cycles,
            report.on_underflow,
            report.on_overflow
        ));
    }
    out
}
