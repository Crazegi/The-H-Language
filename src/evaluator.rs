use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use crate::ast::{BinaryOp, Expr, Instruction, Program, Stmt, UnaryOp};

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Str(String),
    Bool(bool),
    Ref(String),
    Unit,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone)]
struct Frame {
    vars: HashMap<String, Value>,
}

#[derive(Debug)]
pub struct Runtime {
    globals: BTreeMap<String, Value>,
    functions: HashMap<String, crate::ast::Function>,
}

impl Runtime {
    pub fn new(program: &Program) -> Result<Self, RuntimeError> {
        let mut globals = BTreeMap::new();
        for (k, expr) in &program.data {
            let v = eval_const_expr(expr)?;
            globals.insert(k.clone(), v);
        }

        let mut functions = HashMap::new();
        for f in &program.functions {
            functions.insert(f.name.clone(), f.clone());
        }

        Ok(Self { globals, functions })
    }

    pub fn run(&self) -> Result<Value, RuntimeError> {
        let entry = if self.functions.contains_key("main") {
            "main"
        } else {
            self.functions
                .keys()
                .next()
                .ok_or_else(|| RuntimeError::new("No functions to execute"))?
        };

        self.call_function(entry, Vec::new())
    }

    fn call_function(&self, name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let function = self
            .functions
            .get(name)
            .ok_or_else(|| RuntimeError::new(format!("Unknown function `{}`", name)))?;

        if function.params.len() != args.len() {
            return Err(RuntimeError::new(format!(
                "Function `{}` expects {} args, got {}",
                name,
                function.params.len(),
                args.len()
            )));
        }

        let mut frame = Frame {
            vars: HashMap::new(),
        };

        for (param, value) in function.params.iter().zip(args.into_iter()) {
            frame.vars.insert(param.clone(), value);
        }

        let mut ret = Value::Unit;
        for stmt in &function.body {
            match self.execute_stmt(stmt, &mut frame)? {
                Flow::Continue => {}
                Flow::Return(v) => {
                    ret = v;
                    break;
                }
            }
        }

        Ok(ret)
    }

    fn execute_stmt(&self, stmt: &Stmt, frame: &mut Frame) -> Result<Flow, RuntimeError> {
        match stmt {
            Stmt::OwnDecl { name, expr } => {
                let value = self.eval_expr(expr, frame)?;
                frame.vars.insert(name.clone(), value);
                Ok(Flow::Continue)
            }
            Stmt::RefDecl { name, target } => {
                if !frame.vars.contains_key(target) && !self.globals.contains_key(target) {
                    return Err(RuntimeError::new(format!(
                        "Cannot borrow unknown symbol `{}`",
                        target
                    )));
                }
                frame.vars.insert(name.clone(), Value::Ref(target.clone()));
                Ok(Flow::Continue)
            }
            Stmt::Assign { name, expr } => {
                let value = self.eval_expr(expr, frame)?;
                self.assign(frame, name, value)?;
                Ok(Flow::Continue)
            }
            Stmt::Instruction { op, target, rhs } => {
                let rhs_val = self.eval_expr(rhs, frame)?;
                self.execute_instruction(frame, *op, target, rhs_val)?;
                Ok(Flow::Continue)
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                if self.eval_expr(condition, frame)?.as_bool()? {
                    for stmt in then_body {
                        if let Flow::Return(v) = self.execute_stmt(stmt, frame)? {
                            return Ok(Flow::Return(v));
                        }
                    }
                } else {
                    for stmt in else_body {
                        if let Flow::Return(v) = self.execute_stmt(stmt, frame)? {
                            return Ok(Flow::Return(v));
                        }
                    }
                }
                Ok(Flow::Continue)
            }
            Stmt::While { condition, body } => {
                while self.eval_expr(condition, frame)?.as_bool()? {
                    for stmt in body {
                        if let Flow::Return(v) = self.execute_stmt(stmt, frame)? {
                            return Ok(Flow::Return(v));
                        }
                    }
                }
                Ok(Flow::Continue)
            }
            Stmt::PrintBlock(fields) => {
                println!("print:");
                for (key, expr) in fields {
                    let value = self.eval_expr(expr, frame)?;
                    println!("  {}: {}", key, value.render());
                }
                Ok(Flow::Continue)
            }
            Stmt::Return(expr) => {
                let value = match expr {
                    Some(e) => self.eval_expr(e, frame)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(value))
            }
            Stmt::Expr(expr) => {
                let _ = self.eval_expr(expr, frame)?;
                Ok(Flow::Continue)
            }
        }
    }

    fn execute_instruction(
        &self,
        frame: &mut Frame,
        op: Instruction,
        target: &str,
        rhs_val: Value,
    ) -> Result<(), RuntimeError> {
        match op {
            Instruction::Mov => {
                self.assign(frame, target, rhs_val)?;
            }
            Instruction::Add => {
                let current = self.resolve_name(frame, target)?.as_int()?;
                self.assign(frame, target, Value::Int(current + rhs_val.as_int()?))?;
            }
            Instruction::Sub => {
                let current = self.resolve_name(frame, target)?.as_int()?;
                self.assign(frame, target, Value::Int(current - rhs_val.as_int()?))?;
            }
            Instruction::Mul => {
                let current = self.resolve_name(frame, target)?.as_int()?;
                self.assign(frame, target, Value::Int(current * rhs_val.as_int()?))?;
            }
            Instruction::Div => {
                let divisor = rhs_val.as_int()?;
                if divisor == 0 {
                    return Err(RuntimeError::new("Division by zero"));
                }
                let current = self.resolve_name(frame, target)?.as_int()?;
                self.assign(frame, target, Value::Int(current / divisor))?;
            }
            Instruction::Mod => {
                let divisor = rhs_val.as_int()?;
                if divisor == 0 {
                    return Err(RuntimeError::new("Modulo by zero"));
                }
                let current = self.resolve_name(frame, target)?.as_int()?;
                self.assign(frame, target, Value::Int(current % divisor))?;
            }
            Instruction::Cmp => {
                let left = self.resolve_name(frame, target)?;
                let ord = compare_values(&left, &rhs_val)?;
                let cmp = match ord {
                    Ordering::Less => -1,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                };
                frame.vars.insert("cmp".to_string(), Value::Int(cmp));
            }
        }
        Ok(())
    }

    fn eval_expr(&self, expr: &Expr, frame: &Frame) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Number(v) => Ok(Value::Int(*v)),
            Expr::String(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Var(name) => self.resolve_name(frame, name),
            Expr::Unary { op, rhs } => {
                let value = self.eval_expr(rhs, frame)?;
                match op {
                    UnaryOp::Neg => Ok(Value::Int(-value.as_int()?)),
                }
            }
            Expr::Binary { left, op, right } => {
                let l = self.eval_expr(left, frame)?;
                let r = self.eval_expr(right, frame)?;
                eval_binary(l, *op, r)
            }
            Expr::Call { name, args } => {
                let mut evaluated = Vec::with_capacity(args.len());
                for a in args {
                    evaluated.push(self.eval_expr(a, frame)?);
                }
                self.call_function(name, evaluated)
            }
        }
    }

    fn assign(&self, frame: &mut Frame, name: &str, value: Value) -> Result<(), RuntimeError> {
        let target = if let Some(Value::Ref(target)) = frame.vars.get(name) {
            return Err(RuntimeError::new(format!(
                "Cannot assign to ref binding `{}` (points to `{}`)",
                name, target
            )));
        } else {
            name.to_string()
        };

        if frame.vars.contains_key(&target) {
            frame.vars.insert(target, value);
            Ok(())
        } else {
            Err(RuntimeError::new(format!("Unknown local symbol `{}`", name)))
        }
    }

    fn resolve_name(&self, frame: &Frame, name: &str) -> Result<Value, RuntimeError> {
        if let Some(v) = frame.vars.get(name) {
            return match v {
                Value::Ref(target) => self.resolve_name(frame, target),
                _ => Ok(v.clone()),
            };
        }

        if let Some(v) = self.globals.get(name) {
            return Ok(v.clone());
        }

        Err(RuntimeError::new(format!("Unknown symbol `{}`", name)))
    }
}

enum Flow {
    Continue,
    Return(Value),
}

fn eval_const_expr(expr: &Expr) -> Result<Value, RuntimeError> {
    match expr {
        Expr::Number(v) => Ok(Value::Int(*v)),
        Expr::String(v) => Ok(Value::Str(v.clone())),
        Expr::Bool(v) => Ok(Value::Bool(*v)),
        _ => Err(RuntimeError::new(
            "Data section currently supports only scalar literals",
        )),
    }
}

fn eval_binary(left: Value, op: BinaryOp, right: Value) -> Result<Value, RuntimeError> {
    match op {
        BinaryOp::Add => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
            _ => Err(RuntimeError::new("Invalid types for `+`")),
        },
        BinaryOp::Sub => Ok(Value::Int(left.as_int()? - right.as_int()?)),
        BinaryOp::Mul => Ok(Value::Int(left.as_int()? * right.as_int()?)),
        BinaryOp::Div => {
            let divisor = right.as_int()?;
            if divisor == 0 {
                return Err(RuntimeError::new("Division by zero"));
            }
            Ok(Value::Int(left.as_int()? / divisor))
        }
        BinaryOp::Mod => {
            let divisor = right.as_int()?;
            if divisor == 0 {
                return Err(RuntimeError::new("Modulo by zero"));
            }
            Ok(Value::Int(left.as_int()? % divisor))
        }
        BinaryOp::Eq => Ok(Value::Bool(equals(&left, &right))),
        BinaryOp::Ne => Ok(Value::Bool(!equals(&left, &right))),
        BinaryOp::Lt => Ok(Value::Bool(compare_values(&left, &right)? == Ordering::Less)),
        BinaryOp::Lte => Ok(Value::Bool(
            compare_values(&left, &right)? != Ordering::Greater,
        )),
        BinaryOp::Gt => Ok(Value::Bool(compare_values(&left, &right)? == Ordering::Greater)),
        BinaryOp::Gte => Ok(Value::Bool(
            compare_values(&left, &right)? != Ordering::Less,
        )),
    }
}

fn compare_values(left: &Value, right: &Value) -> Result<Ordering, RuntimeError> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Ok(a.cmp(b)),
        _ => Err(RuntimeError::new("Cannot compare values of different types")),
    }
}

fn equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Unit, Value::Unit) => true,
        _ => false,
    }
}

impl Value {
    fn as_int(&self) -> Result<i64, RuntimeError> {
        if let Value::Int(v) = self {
            Ok(*v)
        } else {
            Err(RuntimeError::new("Expected integer value"))
        }
    }

    fn as_bool(&self) -> Result<bool, RuntimeError> {
        match self {
            Value::Bool(v) => Ok(*v),
            Value::Int(v) => Ok(*v != 0),
            _ => Err(RuntimeError::new("Expected boolean-compatible value")),
        }
    }

    pub fn render(&self) -> String {
        match self {
            Value::Int(v) => v.to_string(),
            Value::Str(v) => format!("\"{}\"", v),
            Value::Bool(v) => v.to_string(),
            Value::Ref(v) => format!("&{}", v),
            Value::Unit => "unit".to_string(),
        }
    }
}

pub fn run_program(program: &Program) -> Result<Value, RuntimeError> {
    let runtime = Runtime::new(program)?;
    runtime.run()
}
