use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, Expr, Program, Stmt};
use crate::builtin::builtin_arity;

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
    let mut interrupt_names: HashSet<String> = HashSet::new();
    for f in &program.functions {
        if signatures.insert(f.name.clone(), f.params.len()).is_some() {
            return Err(SemanticError::new(format!(
                "Duplicate function `{}`",
                f.name
            )));
        }
        if f.is_interrupt {
            interrupt_names.insert(f.name.clone());
            if !f.params.is_empty() {
                return Err(SemanticError::new(format!(
                    "Interrupt function `{}` cannot declare parameters",
                    f.name
                )));
            }
        }
    }

    if !signatures.contains_key("main") && program.functions.is_empty() {
        return Err(SemanticError::new(
            "Program must define at least one function (prefer `main`)",
        ));
    }

    let mut port_owners: HashMap<String, String> = HashMap::new();
    let mut owned_ports_by_function: HashMap<String, HashSet<String>> = HashMap::new();
    for f in &program.functions {
        let mut owned_here = Vec::new();
        for stmt in &f.body {
            collect_owned_ports_stmt(stmt, &mut owned_here);
        }

        owned_ports_by_function.insert(f.name.clone(), owned_here.iter().cloned().collect());

        for port in owned_here {
            if let Some(prev_owner) = port_owners.get(&port) {
                if prev_owner != &f.name {
                    return Err(SemanticError::new(format!(
                        "Hardware port `{}` ownership collision: functions `{}` and `{}` both own it",
                        port, prev_owner, f.name
                    )));
                }
            } else {
                port_owners.insert(port, f.name.clone());
            }
        }
    }

    let mut granted_ports_by_interrupt: HashMap<String, HashSet<String>> = HashMap::new();
    for f in &program.functions {
        for stmt in &f.body {
            collect_yield_grants_stmt(
                stmt,
                &f.name,
                &port_owners,
                &interrupt_names,
                &mut granted_ports_by_interrupt,
            )?;
        }
    }

    for f in &program.functions {
        let mut symbols: HashSet<String> = f.params.iter().cloned().collect();
        let mut refs: HashSet<String> = HashSet::new();
        let mut port_access: HashSet<String> = HashSet::new();
        for (port, owner_fn) in &port_owners {
            if owner_fn == &f.name {
                port_access.insert(port.clone());
            }
        }
        if f.is_interrupt {
            if let Some(granted) = granted_ports_by_interrupt.get(&f.name) {
                port_access.extend(granted.iter().cloned());
            }
        }
        for stmt in &f.body {
            analyze_stmt(
                stmt,
                &f.name,
                f.is_interrupt,
                &mut symbols,
                &mut refs,
                &mut port_access,
                &port_owners,
                &owned_ports_by_function,
                &interrupt_names,
                &program.data,
                &signatures,
            )?;
        }
    }

    Ok(())
}

fn analyze_stmt(
    stmt: &Stmt,
    function_name: &str,
    is_interrupt_fn: bool,
    symbols: &mut HashSet<String>,
    refs: &mut HashSet<String>,
    port_access: &mut HashSet<String>,
    port_owners: &HashMap<String, String>,
    owned_ports_by_function: &HashMap<String, HashSet<String>>,
    interrupt_names: &HashSet<String>,
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
            if let Some(port) = memory_target_name(target) {
                if !port_access.contains(port) && !port_owners.contains_key(port) {
                    return Err(SemanticError::new(format!(
                        "Cannot borrow unknown hardware port `{}`",
                        port
                    )));
                }
                port_access.insert(port.to_string());
            } else if !symbols.contains(target) && !data.contains_key(target) {
                return Err(SemanticError::new(format!(
                    "Cannot borrow unknown symbol `{}`",
                    target
                )));
            }
            symbols.insert(name.clone());
            refs.insert(name.clone());
        }
        Stmt::PortOwn { port } => {
            if is_interrupt_fn {
                return Err(SemanticError::new(format!(
                    "Interrupt function `{}` cannot own hardware port `{}` directly; use `yield [port] to {}` from a normal owner function",
                    function_name, port, function_name
                )));
            }
            port_access.insert(port.clone());
        }
        Stmt::PortRef { port } => {
            if !port_access.contains(port) && !port_owners.contains_key(port) {
                return Err(SemanticError::new(format!(
                    "Cannot borrow hardware port `{}` without an owner",
                    port
                )));
            }
            port_access.insert(port.clone());
        }
        Stmt::YieldPort {
            port,
            handler,
            body,
        } => {
            if is_interrupt_fn {
                return Err(SemanticError::new(format!(
                    "Interrupt function `{}` cannot declare `yield` windows",
                    function_name
                )));
            }

            let Some(owned_here) = owned_ports_by_function.get(function_name) else {
                return Err(SemanticError::new(format!(
                    "Internal error: missing ownership map for function `{}`",
                    function_name
                )));
            };

            if !owned_here.contains(port) {
                return Err(SemanticError::new(format!(
                    "Function `{}` can only yield owned hardware ports; `{}` is not owned here",
                    function_name, port
                )));
            }

            if !interrupt_names.contains(handler) {
                return Err(SemanticError::new(format!(
                    "Yield target `{}` must be an `interrupt fn`",
                    handler
                )));
            }

            for s in body {
                analyze_stmt(
                    s,
                    function_name,
                    is_interrupt_fn,
                    symbols,
                    refs,
                    port_access,
                    port_owners,
                    owned_ports_by_function,
                    interrupt_names,
                    data,
                    signatures,
                )?;
            }
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
            if let Some(port) = memory_target_name(target) {
                if !port_access.contains(port) {
                    return Err(SemanticError::new(format!(
                        "Instruction target `{}` requires hardware ownership (`own [{}]` or `ref [{}]`)",
                        target, port, port
                    )));
                }
            } else if !symbols.contains(target) {
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
                analyze_stmt(
                    s,
                    function_name,
                    is_interrupt_fn,
                    symbols,
                    refs,
                    port_access,
                    port_owners,
                    owned_ports_by_function,
                    interrupt_names,
                    data,
                    signatures,
                )?;
            }
            for s in else_body {
                analyze_stmt(
                    s,
                    function_name,
                    is_interrupt_fn,
                    symbols,
                    refs,
                    port_access,
                    port_owners,
                    owned_ports_by_function,
                    interrupt_names,
                    data,
                    signatures,
                )?;
            }
        }
        Stmt::While { condition, body } => {
            analyze_expr(condition, symbols, data, signatures)?;
            for s in body {
                analyze_stmt(
                    s,
                    function_name,
                    is_interrupt_fn,
                    symbols,
                    refs,
                    port_access,
                    port_owners,
                    owned_ports_by_function,
                    interrupt_names,
                    data,
                    signatures,
                )?;
            }
        }
        Stmt::Repeat { times, body } => {
            analyze_expr(times, symbols, data, signatures)?;
            for s in body {
                analyze_stmt(
                    s,
                    function_name,
                    is_interrupt_fn,
                    symbols,
                    refs,
                    port_access,
                    port_owners,
                    owned_ports_by_function,
                    interrupt_names,
                    data,
                    signatures,
                )?;
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
                analyze_stmt(
                    s,
                    function_name,
                    is_interrupt_fn,
                    symbols,
                    refs,
                    port_access,
                    port_owners,
                    owned_ports_by_function,
                    interrupt_names,
                    data,
                    signatures,
                )?;
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

fn is_memory_target(target: &str) -> bool {
    target.starts_with('[') && target.ends_with(']') && target.len() > 2
}

fn memory_target_name(target: &str) -> Option<&str> {
    if is_memory_target(target) {
        Some(&target[1..target.len() - 1])
    } else {
        None
    }
}

fn collect_owned_ports_stmt(stmt: &Stmt, out: &mut Vec<String>) {
    match stmt {
        Stmt::PortOwn { port } => out.push(port.clone()),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_owned_ports_stmt(s, out);
            }
            for s in else_body {
                collect_owned_ports_stmt(s, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::Repeat { body, .. }
        | Stmt::CycleContract { body, .. } => {
            for s in body {
                collect_owned_ports_stmt(s, out);
            }
        }
        Stmt::YieldPort { body, .. } => {
            for s in body {
                collect_owned_ports_stmt(s, out);
            }
        }
        _ => {}
    }
}

fn collect_yield_grants_stmt(
    stmt: &Stmt,
    function_name: &str,
    port_owners: &HashMap<String, String>,
    interrupt_names: &HashSet<String>,
    out: &mut HashMap<String, HashSet<String>>,
) -> Result<(), SemanticError> {
    match stmt {
        Stmt::YieldPort {
            port,
            handler,
            body,
        } => {
            if !interrupt_names.contains(handler) {
                return Err(SemanticError::new(format!(
                    "Yield target `{}` must be an `interrupt fn`",
                    handler
                )));
            }

            let Some(owner) = port_owners.get(port) else {
                return Err(SemanticError::new(format!(
                    "Cannot yield unknown hardware port `{}`",
                    port
                )));
            };

            if owner != function_name {
                return Err(SemanticError::new(format!(
                    "Function `{}` can only yield owned hardware ports; `{}` is owned by `{}`",
                    function_name, port, owner
                )));
            }

            out.entry(handler.clone()).or_default().insert(port.clone());

            for s in body {
                collect_yield_grants_stmt(s, function_name, port_owners, interrupt_names, out)?;
            }
        }
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_yield_grants_stmt(s, function_name, port_owners, interrupt_names, out)?;
            }
            for s in else_body {
                collect_yield_grants_stmt(s, function_name, port_owners, interrupt_names, out)?;
            }
        }
        Stmt::While { body, .. } | Stmt::Repeat { body, .. } | Stmt::CycleContract { body, .. } => {
            for s in body {
                collect_yield_grants_stmt(s, function_name, port_owners, interrupt_names, out)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn is_execute_stmt_shape_supported(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Instruction { .. }
        | Stmt::OwnDecl { .. }
        | Stmt::Assign { .. }
        | Stmt::PortOwn { .. }
        | Stmt::PortRef { .. } => true,
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
