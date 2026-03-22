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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptimizationLevel {
    O0,
    O1,
    O2,
    O3,
}

impl OptimizationLevel {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "0" | "o0" => Some(Self::O0),
            "1" | "o1" => Some(Self::O1),
            "2" | "o2" => Some(Self::O2),
            "3" | "o3" => Some(Self::O3),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub cycle_profile: CycleProfile,
    pub opt_level: OptimizationLevel,
    pub const_folding: bool,
    pub peephole: bool,
    pub fast_math: bool,
    pub strict_cycle_contracts: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            cycle_profile: CycleProfile::Generic,
            opt_level: OptimizationLevel::O2,
            const_folding: true,
            peephole: true,
            fast_math: false,
            strict_cycle_contracts: true,
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
        let mut ctx = FunctionCompiler::new(f.name.clone(), options.clone());
        for stmt in &f.body {
            ctx.compile_stmt(stmt)?;
        }

        ctx.optimize_code();

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
    options: CompileOptions,
    code: Vec<Instruction>,
    temp_counter: usize,
    contract_counter: usize,
    contract_reports: Vec<ContractCompileReport>,
}

impl FunctionCompiler {
    fn new(function_name: String, options: CompileOptions) -> Self {
        Self {
            function_name,
            options,
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
        if should_const_fold(&self.options) {
            if let Some(lit) = fold_expr(expr, self.options.fast_math)? {
                self.emit_folded(lit);
                return Ok(());
            }
        }

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

    fn emit_folded(&mut self, value: FoldValue) {
        match value {
            FoldValue::Int(v) => self.code.push(Instruction::PushInt(v)),
            FoldValue::Bool(v) => self.code.push(Instruction::PushBool(v)),
            FoldValue::Str(v) => self.code.push(Instruction::PushStr(v)),
            FoldValue::Maybe => self.code.push(Instruction::PushMaybe),
        }
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
        let mut const_env: HashMap<String, FoldValue> = HashMap::new();
        for s in body {
            total_cycles = total_cycles
                .checked_add(self.compile_execute_stmt(s, &mut const_env)?)
                .ok_or_else(|| CompileError::new("Cycle count overflow in contract block"))?;
        }

        let mut padded_nops = 0u64;

        if total_cycles > spec.cycles {
            match spec.on_overflow {
                ContractPolicy::CompileError => {
                    if !self.options.strict_cycle_contracts {
                        // relaxed mode ignores strict overflow failure
                    } else {
                    return Err(CompileError::new(format!(
                        "Cycle contract overflow in function `{}` contract #{} (profile: {}): block costs {} cycles but contract allows {}",
                        self.function_name,
                        contract_index,
                        self.options.cycle_profile.as_str(),
                        total_cycles,
                        spec.cycles
                    )));
                    }
                }
                ContractPolicy::PadNop => {
                    // Overflow cannot be solved by padding; keep generated code unchanged.
                }
            }
        } else if total_cycles < spec.cycles {
            let deficit = spec.cycles - total_cycles;
            match spec.on_underflow {
                ContractPolicy::CompileError => {
                    if !self.options.strict_cycle_contracts {
                        // relaxed mode ignores strict underflow failure
                    } else {
                    return Err(CompileError::new(format!(
                        "Cycle contract underflow in function `{}` contract #{} (profile: {}): block costs {} cycles but contract requires {}",
                        self.function_name,
                        contract_index,
                        self.options.cycle_profile.as_str(),
                        total_cycles,
                        spec.cycles
                    )));
                    }
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
            cycle_profile: self.options.cycle_profile,
            declared_cycles: spec.cycles,
            measured_cycles: total_cycles,
            padded_nops,
            final_cycles,
            on_underflow: spec.on_underflow,
            on_overflow: spec.on_overflow,
        });

        Ok(())
    }

    fn compile_execute_stmt(
        &mut self,
        stmt: &Stmt,
        const_env: &mut HashMap<String, FoldValue>,
    ) -> Result<u64, CompileError> {
        match stmt {
            Stmt::Instruction { op, target, rhs } => {
                let cost = stmt_cycle_cost(stmt, self.options.cycle_profile)?;
                self.compile_stmt(stmt)?;

                if !is_memory_target(target) {
                    match op {
                        AstInstruction::Mov => {
                            if let Some(value) = eval_execute_const_expr(
                                rhs,
                                const_env,
                                self.options.fast_math,
                            )? {
                                const_env.insert(target.clone(), value);
                            } else {
                                const_env.remove(target);
                            }
                        }
                        AstInstruction::Cmp => {
                            const_env.insert("cmp".to_string(), FoldValue::Int(0));
                            const_env.remove(target);
                        }
                        AstInstruction::Add
                        | AstInstruction::Sub
                        | AstInstruction::Mul
                        | AstInstruction::Div
                        | AstInstruction::Mod => {
                            const_env.remove(target);
                        }
                    }
                }

                Ok(cost)
            }
            Stmt::OwnDecl { name, expr } => {
                self.compile_stmt(stmt)?;
                let mut cost = expr_cycle_cost(expr, self.options.cycle_profile)?;
                cost = cost
                    .checked_add(1)
                    .ok_or_else(|| CompileError::new("Cycle count overflow in execute block"))?;

                if let Some(value) =
                    eval_execute_const_expr(expr, const_env, self.options.fast_math)?
                {
                    const_env.insert(name.clone(), value);
                } else {
                    const_env.remove(name);
                }
                Ok(cost)
            }
            Stmt::Assign { name, expr } => {
                self.compile_stmt(stmt)?;
                let mut cost = expr_cycle_cost(expr, self.options.cycle_profile)?;
                cost = cost
                    .checked_add(1)
                    .ok_or_else(|| CompileError::new("Cycle count overflow in execute block"))?;

                if let Some(value) =
                    eval_execute_const_expr(expr, const_env, self.options.fast_math)?
                {
                    const_env.insert(name.clone(), value);
                } else {
                    const_env.remove(name);
                }
                Ok(cost)
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let Some(cond) =
                    eval_execute_const_expr(condition, const_env, self.options.fast_math)?
                else {
                    return Err(CompileError::new(
                        "Cycle contract execute `if` condition must be compile-time constant",
                    ));
                };

                let choose_then = fold_value_as_bool(&cond)?;
                let chosen = if choose_then { then_body } else { else_body };

                let mut total = 0u64;
                for s in chosen {
                    total = total
                        .checked_add(self.compile_execute_stmt(s, const_env)?)
                        .ok_or_else(|| CompileError::new("Cycle count overflow in execute block"))?;
                }
                Ok(total)
            }
            Stmt::Repeat { times, body } => {
                let Some(reps) = eval_execute_const_expr(times, const_env, self.options.fast_math)?
                else {
                    return Err(CompileError::new(
                        "Cycle contract execute `repeat` count must be compile-time constant",
                    ));
                };

                let reps = fold_value_as_non_negative_int(&reps)?;
                let mut total = 0u64;
                for _ in 0..reps {
                    for s in body {
                        total = total
                            .checked_add(self.compile_execute_stmt(s, const_env)?)
                            .ok_or_else(|| {
                                CompileError::new("Cycle count overflow in execute block")
                            })?;
                    }
                }
                Ok(total)
            }
            _ => Err(CompileError::new(
                "Cycle contract execute block supports deterministic statements only",
            )),
        }
    }

    fn optimize_code(&mut self) {
        if !should_peephole(&self.options) {
            return;
        }

        let mut out = Vec::with_capacity(self.code.len());
        let mut i = 0usize;
        while i < self.code.len() {
            if i + 1 < self.code.len()
                && is_push_literal(&self.code[i])
                && matches!(self.code[i + 1], Instruction::Pop)
            {
                i += 2;
                continue;
            }

            if matches!(self.code[i], Instruction::Jump(target) if target == i + 1) {
                i += 1;
                continue;
            }

            out.push(self.code[i].clone());
            i += 1;
        }
        self.code = out;
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

fn expr_cycle_cost(expr: &Expr, profile: CycleProfile) -> Result<u64, CompileError> {
    match expr {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Maybe | Expr::Var(_) => Ok(1),
        Expr::Unary { rhs, .. } => {
            let rhs_cost = expr_cycle_cost(rhs, profile)?;
            rhs_cost
                .checked_add(1)
                .ok_or_else(|| CompileError::new("Cycle count overflow in expression"))
        }
        Expr::Binary { left, op, right } => {
            let left_cost = expr_cycle_cost(left, profile)?;
            let right_cost = expr_cycle_cost(right, profile)?;
            let op_cost = match op {
                BinaryOp::Mul => match profile {
                    CycleProfile::Generic => 2,
                    CycleProfile::AvrLike => 3,
                    CycleProfile::CortexM0Like => 1,
                },
                BinaryOp::Div | BinaryOp::Mod => match profile {
                    CycleProfile::Generic => 2,
                    CycleProfile::AvrLike => 4,
                    CycleProfile::CortexM0Like => 8,
                },
                _ => 1,
            };

            left_cost
                .checked_add(right_cost)
                .and_then(|v| v.checked_add(op_cost))
                .ok_or_else(|| CompileError::new("Cycle count overflow in expression"))
        }
        Expr::Call { .. } => Err(CompileError::new(
            "Cycle contract execute block does not allow function calls",
        )),
    }
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

#[derive(Debug, Clone)]
enum FoldValue {
    Int(i64),
    Bool(bool),
    Str(String),
    Maybe,
}

fn fold_expr(expr: &Expr, fast_math: bool) -> Result<Option<FoldValue>, CompileError> {
    match expr {
        Expr::Number(v) => Ok(Some(FoldValue::Int(*v))),
        Expr::String(v) => Ok(Some(FoldValue::Str(v.clone()))),
        Expr::Bool(v) => Ok(Some(FoldValue::Bool(*v))),
        Expr::Maybe => Ok(Some(FoldValue::Maybe)),
        Expr::Var(_) | Expr::Call { .. } => Ok(None),
        Expr::Unary { op, rhs } => {
            let Some(rhs) = fold_expr(rhs, fast_math)? else {
                return Ok(None);
            };
            match (op, rhs) {
                (UnaryOp::Neg, FoldValue::Int(v)) => Ok(Some(FoldValue::Int(-v))),
                (UnaryOp::Not, FoldValue::Bool(v)) => Ok(Some(FoldValue::Bool(!v))),
                (UnaryOp::Not, FoldValue::Maybe) => Ok(Some(FoldValue::Maybe)),
                _ => Ok(None),
            }
        }
        Expr::Binary { left, op, right } => {
            let Some(left) = fold_expr(left, fast_math)? else {
                return Ok(None);
            };
            let Some(right) = fold_expr(right, fast_math)? else {
                return Ok(None);
            };

            fold_binary(left, *op, right, fast_math)
        }
    }
}

fn fold_binary(
    left: FoldValue,
    op: BinaryOp,
    right: FoldValue,
    fast_math: bool,
) -> Result<Option<FoldValue>, CompileError> {
    use BinaryOp::*;
    match (left, op, right) {
        (FoldValue::Int(a), Add, FoldValue::Int(b)) => Ok(Some(FoldValue::Int(a + b))),
        (FoldValue::Int(a), Sub, FoldValue::Int(b)) => Ok(Some(FoldValue::Int(a - b))),
        (FoldValue::Int(a), Mul, FoldValue::Int(b)) => Ok(Some(FoldValue::Int(a * b))),
        (FoldValue::Int(a), Div, FoldValue::Int(b)) => {
            if b == 0 {
                if fast_math {
                    Ok(Some(FoldValue::Int(0)))
                } else {
                    Err(CompileError::new("Constant division by zero during folding"))
                }
            } else {
                Ok(Some(FoldValue::Int(a / b)))
            }
        }
        (FoldValue::Int(a), Mod, FoldValue::Int(b)) => {
            if b == 0 {
                if fast_math {
                    Ok(Some(FoldValue::Int(0)))
                } else {
                    Err(CompileError::new("Constant modulo by zero during folding"))
                }
            } else {
                Ok(Some(FoldValue::Int(a % b)))
            }
        }
        (FoldValue::Int(a), Eq, FoldValue::Int(b)) => Ok(Some(FoldValue::Bool(a == b))),
        (FoldValue::Int(a), Ne, FoldValue::Int(b)) => Ok(Some(FoldValue::Bool(a != b))),
        (FoldValue::Int(a), Lt, FoldValue::Int(b)) => Ok(Some(FoldValue::Bool(a < b))),
        (FoldValue::Int(a), Lte, FoldValue::Int(b)) => Ok(Some(FoldValue::Bool(a <= b))),
        (FoldValue::Int(a), Gt, FoldValue::Int(b)) => Ok(Some(FoldValue::Bool(a > b))),
        (FoldValue::Int(a), Gte, FoldValue::Int(b)) => Ok(Some(FoldValue::Bool(a >= b))),
        (FoldValue::Bool(a), Eq, FoldValue::Bool(b)) => Ok(Some(FoldValue::Bool(a == b))),
        (FoldValue::Bool(a), Ne, FoldValue::Bool(b)) => Ok(Some(FoldValue::Bool(a != b))),
        (FoldValue::Bool(a), And, FoldValue::Bool(b)) => Ok(Some(FoldValue::Bool(a && b))),
        (FoldValue::Bool(a), Or, FoldValue::Bool(b)) => Ok(Some(FoldValue::Bool(a || b))),
        (FoldValue::Bool(a), Xor, FoldValue::Bool(b)) => Ok(Some(FoldValue::Bool(a ^ b))),
        (FoldValue::Maybe, And, FoldValue::Bool(false))
        | (FoldValue::Bool(false), And, FoldValue::Maybe) => Ok(Some(FoldValue::Bool(false))),
        (FoldValue::Maybe, Or, FoldValue::Bool(true))
        | (FoldValue::Bool(true), Or, FoldValue::Maybe) => Ok(Some(FoldValue::Bool(true))),
        (FoldValue::Maybe, Eq, FoldValue::Maybe) => Ok(Some(FoldValue::Bool(true))),
        (FoldValue::Str(a), Add, FoldValue::Str(b)) => Ok(Some(FoldValue::Str(format!("{}{}", a, b)))),
        _ => Ok(None),
    }
}

fn eval_execute_const_expr(
    expr: &Expr,
    const_env: &HashMap<String, FoldValue>,
    fast_math: bool,
) -> Result<Option<FoldValue>, CompileError> {
    match expr {
        Expr::Number(v) => Ok(Some(FoldValue::Int(*v))),
        Expr::String(v) => Ok(Some(FoldValue::Str(v.clone()))),
        Expr::Bool(v) => Ok(Some(FoldValue::Bool(*v))),
        Expr::Maybe => Ok(Some(FoldValue::Maybe)),
        Expr::Var(name) => Ok(const_env.get(name).cloned()),
        Expr::Call { .. } => Ok(None),
        Expr::Unary { op, rhs } => {
            let Some(rhs) = eval_execute_const_expr(rhs, const_env, fast_math)? else {
                return Ok(None);
            };
            match (op, rhs) {
                (UnaryOp::Neg, FoldValue::Int(v)) => Ok(Some(FoldValue::Int(-v))),
                (UnaryOp::Not, FoldValue::Bool(v)) => Ok(Some(FoldValue::Bool(!v))),
                (UnaryOp::Not, FoldValue::Maybe) => Ok(Some(FoldValue::Maybe)),
                _ => Ok(None),
            }
        }
        Expr::Binary { left, op, right } => {
            let Some(left) = eval_execute_const_expr(left, const_env, fast_math)? else {
                return Ok(None);
            };
            let Some(right) = eval_execute_const_expr(right, const_env, fast_math)? else {
                return Ok(None);
            };
            fold_binary(left, *op, right, fast_math)
        }
    }
}

fn fold_value_as_bool(value: &FoldValue) -> Result<bool, CompileError> {
    match value {
        FoldValue::Bool(v) => Ok(*v),
        FoldValue::Int(v) => Ok(*v != 0),
        FoldValue::Maybe => Ok(false),
        _ => Err(CompileError::new(
            "Cycle contract execute `if` expects boolean-compatible constant",
        )),
    }
}

fn fold_value_as_non_negative_int(value: &FoldValue) -> Result<u64, CompileError> {
    match value {
        FoldValue::Int(v) if *v >= 0 => Ok(*v as u64),
        FoldValue::Int(_) => Err(CompileError::new(
            "Cycle contract execute `repeat` count must be non-negative",
        )),
        _ => Err(CompileError::new(
            "Cycle contract execute `repeat` count must be an integer constant",
        )),
    }
}

fn is_push_literal(ins: &Instruction) -> bool {
    matches!(
        ins,
        Instruction::PushInt(_)
            | Instruction::PushStr(_)
            | Instruction::PushBool(_)
            | Instruction::PushMaybe
            | Instruction::PushUnit
    )
}

fn should_const_fold(options: &CompileOptions) -> bool {
    options.const_folding && options.opt_level >= OptimizationLevel::O1
}

fn should_peephole(options: &CompileOptions) -> bool {
    options.peephole && options.opt_level >= OptimizationLevel::O2
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
