use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, Expr, Program, Stmt};

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub message: String,
}

impl SemanticError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SemanticError {}

pub fn analyze(program: &Program) -> Result<(), SemanticError> {
    let mut signatures: HashMap<String, usize> = HashMap::new();
    for f in &program.functions {
        if signatures.insert(f.name.clone(), f.params.len()).is_some() {
            return Err(SemanticError::new(format!(
                "Duplicate function `{}`",
                f.name
            )));
        }
    }

    if !signatures.contains_key("main") && program.functions.is_empty() {
        return Err(SemanticError::new(
            "Program must define at least one function (prefer `main`)",
        ));
    }

    for f in &program.functions {
        let mut symbols: HashSet<String> = f.params.iter().cloned().collect();
        let mut refs: HashSet<String> = HashSet::new();
        for stmt in &f.body {
            analyze_stmt(stmt, &mut symbols, &mut refs, &program.data, &signatures)?;
        }
    }

    Ok(())
}

fn analyze_stmt(
    stmt: &Stmt,
    symbols: &mut HashSet<String>,
    refs: &mut HashSet<String>,
    data: &std::collections::BTreeMap<String, Expr>,
    signatures: &HashMap<String, usize>,
) -> Result<(), SemanticError> {
    match stmt {
        Stmt::OwnDecl { name, expr } => {
            if symbols.contains(name) {
                return Err(SemanticError::new(format!(
                    "`{}` already declared in this function",
                    name
                )));
            }
            analyze_expr(expr, symbols, data, signatures)?;
            symbols.insert(name.clone());
        }
        Stmt::RefDecl { name, target } => {
            if symbols.contains(name) {
                return Err(SemanticError::new(format!("`{}` already declared", name)));
            }
            if !symbols.contains(target) && !data.contains_key(target) {
                return Err(SemanticError::new(format!(
                    "Cannot borrow unknown symbol `{}`",
                    target
                )));
            }
            symbols.insert(name.clone());
            refs.insert(name.clone());
        }
        Stmt::Assign { name, expr } => {
            if !symbols.contains(name) {
                return Err(SemanticError::new(format!(
                    "Assignment to undeclared symbol `{}`",
                    name
                )));
            }
            if refs.contains(name) {
                return Err(SemanticError::new(format!(
                    "Cannot assign to reference binding `{}`",
                    name
                )));
            }
            analyze_expr(expr, symbols, data, signatures)?;
        }
        Stmt::Instruction { target, rhs, .. } => {
            if !is_memory_target(target) && !symbols.contains(target) {
                return Err(SemanticError::new(format!(
                    "Instruction target `{}` is undeclared",
                    target
                )));
            }
            if !is_memory_target(target) && refs.contains(target) {
                return Err(SemanticError::new(format!(
                    "Instruction target `{}` cannot be a reference",
                    target
                )));
            }
            analyze_expr(rhs, symbols, data, signatures)?;
        }
        Stmt::If {
            condition,
            then_body,
            else_body,
        } => {
            analyze_expr(condition, symbols, data, signatures)?;
            for s in then_body {
                analyze_stmt(s, symbols, refs, data, signatures)?;
            }
            for s in else_body {
                analyze_stmt(s, symbols, refs, data, signatures)?;
            }
        }
        Stmt::While { condition, body } => {
            analyze_expr(condition, symbols, data, signatures)?;
            for s in body {
                analyze_stmt(s, symbols, refs, data, signatures)?;
            }
        }
        Stmt::Repeat { times, body } => {
            analyze_expr(times, symbols, data, signatures)?;
            for s in body {
                analyze_stmt(s, symbols, refs, data, signatures)?;
            }
        }
        Stmt::CycleContract { spec, body } => {
            if spec.cycles == 0 {
                return Err(SemanticError::new("Cycle contract `cycles` must be > 0"));
            }
            for s in body {
                if !is_execute_stmt_shape_supported(s) {
                    return Err(SemanticError::new(
                        "Cycle contract execute block supports deterministic statements only: instruction, own/assign, if, repeat",
                    ));
                }
                analyze_stmt(s, symbols, refs, data, signatures)?;
            }
        }
        Stmt::PrintBlock(fields) => {
            for (_, expr) in fields {
                analyze_expr(expr, symbols, data, signatures)?;
            }
        }
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                analyze_expr(e, symbols, data, signatures)?;
            }
        }
        Stmt::Expr(expr) => analyze_expr(expr, symbols, data, signatures)?,
    }
    Ok(())
}

fn analyze_expr(
    expr: &Expr,
    symbols: &HashSet<String>,
    data: &std::collections::BTreeMap<String, Expr>,
    signatures: &HashMap<String, usize>,
) -> Result<(), SemanticError> {
    match expr {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Maybe => Ok(()),
        Expr::Var(name) => {
            if symbols.contains(name) || data.contains_key(name) {
                Ok(())
            } else {
                Err(SemanticError::new(format!(
                    "Use of undeclared symbol `{}`",
                    name
                )))
            }
        }
        Expr::Unary { rhs, .. } => analyze_expr(rhs, symbols, data, signatures),
        Expr::Binary { left, right, op } => {
            analyze_expr(left, symbols, data, signatures)?;
            analyze_expr(right, symbols, data, signatures)?;
            if matches!(op, BinaryOp::Div | BinaryOp::Mod)
                && matches!(right.as_ref(), Expr::Number(0))
            {
                return Err(SemanticError::new("Division/modulo by literal zero"));
            }
            Ok(())
        }
        Expr::Call { name, args } => {
            let expected = if let Some(v) = signatures.get(name) {
                *v
            } else if let Some(v) = builtin_arity(name) {
                v
            } else {
                return Err(SemanticError::new(format!(
                    "Call to unknown function `{}`",
                    name
                )));
            };

            if expected != args.len() {
                return Err(SemanticError::new(format!(
                    "Function `{}` expects {} args, got {}",
                    name,
                    expected,
                    args.len()
                )));
            }
            for a in args {
                analyze_expr(a, symbols, data, signatures)?;
            }
            Ok(())
        }
    }
}

fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "abs" => Some(1),
        "sqrt" => Some(1),
        "min" => Some(2),
        "max" => Some(2),
        "pow" => Some(2),
        "clamp" => Some(3),
        "len" => Some(1),
        "upper" => Some(1),
        "lower" => Some(1),
        "contains" => Some(2),
        "phase" => Some(2),
        "collapse" => Some(1),
        _ => None,
    }
}

fn is_memory_target(target: &str) -> bool {
    target.starts_with('[') && target.ends_with(']') && target.len() > 2
}

fn is_execute_stmt_shape_supported(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Instruction { .. } | Stmt::OwnDecl { .. } | Stmt::Assign { .. } => true,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().all(is_execute_stmt_shape_supported)
                && else_body.iter().all(is_execute_stmt_shape_supported)
        }
        Stmt::Repeat { body, .. } => body.iter().all(is_execute_stmt_shape_supported),
        _ => false,
    }
}
