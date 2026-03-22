use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, Expr, Program, Stmt};
use crate::builtin::{
    builtin_accepts_arity, builtin_arity, count_format_placeholders, is_known_builtin_module,
    normalize_builtin_name,
};
use crate::token::Span;

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub source_line: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SemanticWarning {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub source_line: Option<String>,
}

impl SemanticWarning {
    fn at(span: Span, source: &str, message: impl Into<String>) -> Self {
        Self {
            line: span.line,
            column: span.column,
            message: message.into(),
            source_line: source_line_for(source, span.line),
        }
    }
}

impl SemanticError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            line: 0,
            column: 0,
            message: message.into(),
            source_line: None,
        }
    }

    fn at(span: Span, source: &str, message: impl Into<String>) -> Self {
        Self {
            line: span.line,
            column: span.column,
            message: message.into(),
            source_line: source_line_for(source, span.line),
        }
    }
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 && self.column > 0 {
            write!(f, "{} at {}:{}", self.message, self.line, self.column)?;
            if let Some(line) = &self.source_line {
                write!(
                    f,
                    "\n{}\n{}^",
                    line,
                    " ".repeat(self.column.saturating_sub(1))
                )?;
            }
            Ok(())
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::fmt::Display for SemanticWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 && self.column > 0 {
            write!(f, "warning: {} at {}:{}", self.message, self.line, self.column)?;
            if let Some(line) = &self.source_line {
                write!(
                    f,
                    "\n{}\n{}^",
                    line,
                    " ".repeat(self.column.saturating_sub(1))
                )?;
            }
            Ok(())
        } else {
            write!(f, "warning: {}", self.message)
        }
    }
}

impl std::error::Error for SemanticError {}

pub fn analyze(program: &Program) -> Result<(), SemanticError> {
    analyze_with_warnings(program).map(|_| ())
}

pub fn analyze_with_warnings(program: &Program) -> Result<Vec<SemanticWarning>, SemanticError> {
    let source = program.source.as_str();
    let mut warnings = Vec::new();

    let mut imported_modules: HashSet<String> = HashSet::new();
    for (idx, module) in program.imports.iter().enumerate() {
        let span = program
            .import_spans
            .get(idx)
            .copied()
            .unwrap_or(Span { line: 1, column: 1 });
        if !is_known_builtin_module(module) {
            return Err(SemanticError::at(
                span,
                source,
                format!("Unknown import module `{}`", module),
            ));
        }
        if !imported_modules.insert(module.clone()) {
            return Err(SemanticError::at(
                span,
                source,
                format!("Duplicate import `{}`", module),
            ));
        }
    }

    let mut signatures: HashMap<String, usize> = HashMap::new();
    let mut interrupt_names: HashSet<String> = HashSet::new();
    for f in &program.functions {
        if signatures.insert(f.name.clone(), f.params.len()).is_some() {
            return Err(SemanticError::at(
                f.span,
                source,
                format!("Duplicate function `{}`", f.name),
            ));
        }
        if f.is_interrupt {
            interrupt_names.insert(f.name.clone());
            if !f.params.is_empty() {
                return Err(SemanticError::at(
                    f.span,
                    source,
                    format!(
                        "Interrupt function `{}` cannot declare parameters",
                        f.name
                    ),
                ));
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
    let mut owned_port_spans_by_function: HashMap<String, HashMap<String, Span>> = HashMap::new();
    for f in &program.functions {
        let mut owned_here = Vec::new();
        let mut owned_port_spans = HashMap::new();
        for stmt in &f.body {
            collect_owned_ports_stmt(stmt, &mut owned_here);
            collect_owned_port_spans_stmt(stmt, &mut owned_port_spans);
        }

        owned_ports_by_function.insert(f.name.clone(), owned_here.iter().cloned().collect());
        owned_port_spans_by_function.insert(f.name.clone(), owned_port_spans);

        for port in owned_here {
            if let Some(prev_owner) = port_owners.get(&port) {
                if prev_owner != &f.name {
                    return Err(SemanticError::at(
                        f.span,
                        source,
                        format!(
                            "Hardware port `{}` ownership collision: functions `{}` and `{}` both own it",
                            port, prev_owner, f.name
                        ),
                    ));
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
                source,
            )?;
        }
    }

    for f in &program.functions {
        let mut symbols: HashSet<String> = f.params.iter().cloned().collect();
        let mut refs: HashSet<String> = HashSet::new();
        let mut declared_symbols: HashMap<String, Span> = HashMap::new();
        for p in &f.params {
            declared_symbols.insert(p.clone(), f.span);
        }
        let mut used_symbols: HashSet<String> = HashSet::new();
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
                &imported_modules,
                &mut declared_symbols,
                &mut used_symbols,
                source,
            )?;
        }

        for (symbol, span) in declared_symbols {
            if symbol.starts_with('_') {
                continue;
            }
            if !used_symbols.contains(&symbol) {
                warnings.push(SemanticWarning::at(
                    span,
                    source,
                    format!("Variable `{}` declared but never used", symbol),
                ));
            }
        }

        let mut written_ports = HashSet::new();
        for stmt in &f.body {
            collect_written_ports_stmt(stmt, &mut written_ports);
        }
        if let Some(owned_port_spans) = owned_port_spans_by_function.get(&f.name) {
            for (port, span) in owned_port_spans {
                if !written_ports.contains(port) {
                    warnings.push(SemanticWarning::at(
                        *span,
                        source,
                        format!("Hardware port `{}` is owned but never written to", port),
                    ));
                }
            }
        }
    }

    Ok(warnings)
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
    imported_modules: &HashSet<String>,
    declared_symbols: &mut HashMap<String, Span>,
    used_symbols: &mut HashSet<String>,
    source: &str,
) -> Result<(), SemanticError> {
    let stmt_span = stmt.span();
    match stmt {
        Stmt::OwnDecl { name, expr, .. } => {
            if symbols.contains(name) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("`{}` already declared in this function", name),
                ));
            }
            analyze_expr(
                expr,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
            symbols.insert(name.clone());
            declared_symbols.insert(name.clone(), stmt_span);
        }
        Stmt::RefDecl { name, target, .. } => {
            if symbols.contains(name) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("`{}` already declared", name),
                ));
            }
            if let Some(port) = memory_target_name(target) {
                if !port_access.contains(port) && !port_owners.contains_key(port) {
                    return Err(SemanticError::at(
                        stmt_span,
                        source,
                        format!("Cannot borrow unknown hardware port `{}`", port),
                    ));
                }
                port_access.insert(port.to_string());
            } else if !symbols.contains(target) && !data.contains_key(target) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Cannot borrow unknown symbol `{}`", target),
                ));
            }
            if symbols.contains(target) {
                used_symbols.insert(target.clone());
            }
            symbols.insert(name.clone());
            refs.insert(name.clone());
            declared_symbols.insert(name.clone(), stmt_span);
        }
        Stmt::PortOwn { port, .. } => {
            if is_interrupt_fn {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!(
                        "Interrupt function `{}` cannot own hardware port `{}` directly; use `yield [port] to {}` from a normal owner function",
                        function_name, port, function_name
                    ),
                ));
            }
            port_access.insert(port.clone());
        }
        Stmt::PortRef { port, .. } => {
            if !port_access.contains(port) && !port_owners.contains_key(port) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Cannot borrow hardware port `{}` without an owner", port),
                ));
            }
            port_access.insert(port.clone());
        }
        Stmt::YieldPort {
            port,
            handler,
            body,
            ..
        } => {
            if is_interrupt_fn {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!(
                        "Interrupt function `{}` cannot declare `yield` windows",
                        function_name
                    ),
                ));
            }

            let Some(owned_here) = owned_ports_by_function.get(function_name) else {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!(
                        "Internal error: missing ownership map for function `{}`",
                        function_name
                    ),
                ));
            };

            if !owned_here.contains(port) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!(
                        "Function `{}` can only yield owned hardware ports; `{}` is not owned here",
                        function_name, port
                    ),
                ));
            }

            if !interrupt_names.contains(handler) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Yield target `{}` must be an `interrupt fn`", handler),
                ));
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::Assign { name, expr, .. } => {
            if !symbols.contains(name) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Assignment to undeclared symbol `{}`", name),
                ));
            }
            if refs.contains(name) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Cannot assign to reference binding `{}`", name),
                ));
            }
            analyze_expr(
                expr,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
        }
        Stmt::Instruction {
            op,
            target,
            rhs,
            ..
        } => {
            if let Some(port) = memory_target_name(target) {
                if !port_access.contains(port) {
                    return Err(SemanticError::at(
                        stmt_span,
                        source,
                        format!(
                            "Instruction target `{}` requires hardware ownership (`own [{}]` or `ref [{}]`)",
                            target, port, port
                        ),
                    ));
                }
            } else if !symbols.contains(target) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Instruction target `{}` is undeclared", target),
                ));
            }
            if !is_memory_target(target) && refs.contains(target) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("Instruction target `{}` cannot be a reference", target),
                ));
            }
            if !is_memory_target(target) && !matches!(op, crate::ast::Instruction::Mov) {
                used_symbols.insert(target.clone());
            }
            analyze_expr(
                rhs,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
        }
        Stmt::If {
            condition,
            then_body,
            else_body,
            ..
        } => {
            analyze_expr(
                condition,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::While {
            condition, body, ..
        } => {
            analyze_expr(
                condition,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::Repeat { times, body, .. } => {
            analyze_expr(
                times,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::For {
            name,
            iterable,
            body,
            ..
        } => {
            if symbols.contains(name) {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    format!("`{}` already declared in this function", name),
                ));
            }
            analyze_expr(
                iterable,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
            symbols.insert(name.clone());
            declared_symbols.insert(name.clone(), stmt_span);
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::CycleContract { spec, body, .. } => {
            if spec.cycles == 0 {
                return Err(SemanticError::at(
                    stmt_span,
                    source,
                    "Cycle contract `cycles` must be > 0",
                ));
            }
            for s in body {
                if !is_execute_stmt_shape_supported(s) {
                    return Err(SemanticError::at(
                        s.span(),
                        source,
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
                    imported_modules,
                    declared_symbols,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::PrintBlock { fields, .. } => {
            for (_, expr) in fields {
                analyze_expr(
                    expr,
                    symbols,
                    data,
                    signatures,
                    imported_modules,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::Return { expr, .. } => {
            if let Some(e) = expr {
                analyze_expr(
                    e,
                    symbols,
                    data,
                    signatures,
                    imported_modules,
                    used_symbols,
                    source,
                )?;
            }
        }
        Stmt::Expr { expr, .. } => {
            analyze_expr(
                expr,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?
        }
    }
    Ok(())
}

fn analyze_expr(
    expr: &Expr,
    symbols: &HashSet<String>,
    data: &std::collections::BTreeMap<String, Expr>,
    signatures: &HashMap<String, usize>,
    imported_modules: &HashSet<String>,
    used_symbols: &mut HashSet<String>,
    source: &str,
) -> Result<(), SemanticError> {
    match expr {
        Expr::Number(_, _) | Expr::String(_, _) | Expr::Bool(_, _) | Expr::Maybe(_) => Ok(()),
        Expr::Var(name, span) => {
            if symbols.contains(name) || data.contains_key(name) {
                if symbols.contains(name) {
                    used_symbols.insert(name.clone());
                }
                Ok(())
            } else {
                Err(SemanticError::at(
                    *span,
                    source,
                    format!("Use of undeclared symbol `{}`", name),
                ))
            }
        }
        Expr::Unary { rhs, .. } => {
            analyze_expr(
                rhs,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )
        }
        Expr::Binary {
            left, right, op, ..
        } => {
            analyze_expr(
                left,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
            analyze_expr(
                right,
                symbols,
                data,
                signatures,
                imported_modules,
                used_symbols,
                source,
            )?;
            if matches!(op, BinaryOp::Div | BinaryOp::Mod)
                && matches!(right.as_ref(), Expr::Number(0, _))
            {
                return Err(SemanticError::at(
                    right.span(),
                    source,
                    "Division/modulo by literal zero",
                ));
            }
            Ok(())
        }
        Expr::Call { name, args, span } => {
            if let Some((module, _)) = name.split_once('.') {
                if !imported_modules.contains(module) {
                    return Err(SemanticError::at(
                        *span,
                        source,
                        format!("Module `{}` is not imported (add `import {}`)", module, module),
                    ));
                }
            }

            let mut normalized_builtin_name: Option<String> = None;
            let expected = if let Some(v) = signatures.get(name) {
                *v
            } else if let Some(normalized) = normalize_builtin_name(name) {
                normalized_builtin_name = Some(normalized.clone());
                builtin_arity(&normalized).unwrap_or(usize::MAX)
            } else {
                return Err(SemanticError::at(
                    *span,
                    source,
                    format!("Call to unknown function `{}`", name),
                ));
            };

            if let Some(normalized) = normalized_builtin_name.as_deref() {
                if !builtin_accepts_arity(normalized, args.len()) {
                    return Err(SemanticError::at(
                        *span,
                        source,
                        format!("Function `{}` expects {} args, got {}", name, expected, args.len()),
                    ));
                }

                if normalized == "format" {
                    if let Some(Expr::String(template, _)) = args.first() {
                        let placeholders = count_format_placeholders(template);
                        let provided = args.len().saturating_sub(1);
                        if placeholders != provided {
                            return Err(SemanticError::at(
                                *span,
                                source,
                                format!(
                                    "Function `{}` expects {} value args for {} placeholder(s), got {}",
                                    name, placeholders, placeholders, provided
                                ),
                            ));
                        }
                    }
                }
            } else if expected != args.len() {
                return Err(SemanticError::at(
                    *span,
                    source,
                    format!("Function `{}` expects {} args, got {}", name, expected, args.len()),
                ));
            }
            for a in args {
                analyze_expr(
                    a,
                    symbols,
                    data,
                    signatures,
                    imported_modules,
                    used_symbols,
                    source,
                )?;
            }
            Ok(())
        }
    }
}

fn collect_owned_port_spans_stmt(stmt: &Stmt, out: &mut HashMap<String, Span>) {
    match stmt {
        Stmt::PortOwn { port, span } => {
            out.entry(port.clone()).or_insert(*span);
        }
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_owned_port_spans_stmt(s, out);
            }
            for s in else_body {
                collect_owned_port_spans_stmt(s, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::Repeat { body, .. }
        | Stmt::For { body, .. }
        | Stmt::CycleContract { body, .. }
        | Stmt::YieldPort { body, .. } => {
            for s in body {
                collect_owned_port_spans_stmt(s, out);
            }
        }
        _ => {}
    }
}

fn collect_written_ports_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Instruction { target, .. } => {
            if let Some(port) = memory_target_name(target) {
                out.insert(port.to_string());
            }
        }
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_written_ports_stmt(s, out);
            }
            for s in else_body {
                collect_written_ports_stmt(s, out);
            }
        }
        Stmt::While { body, .. }
        | Stmt::Repeat { body, .. }
        | Stmt::For { body, .. }
        | Stmt::CycleContract { body, .. }
        | Stmt::YieldPort { body, .. } => {
            for s in body {
                collect_written_ports_stmt(s, out);
            }
        }
        _ => {}
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
        Stmt::PortOwn { port, .. } => out.push(port.clone()),
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
        | Stmt::For { body, .. }
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
    source: &str,
) -> Result<(), SemanticError> {
    match stmt {
        Stmt::YieldPort {
            port,
            handler,
            body,
            ..
        } => {
            if !interrupt_names.contains(handler) {
                return Err(SemanticError::at(
                    stmt.span(),
                    source,
                    format!("Yield target `{}` must be an `interrupt fn`", handler),
                ));
            }

            let Some(owner) = port_owners.get(port) else {
                return Err(SemanticError::at(
                    stmt.span(),
                    source,
                    format!("Cannot yield unknown hardware port `{}`", port),
                ));
            };

            if owner != function_name {
                return Err(SemanticError::at(
                    stmt.span(),
                    source,
                    format!(
                        "Function `{}` can only yield owned hardware ports; `{}` is owned by `{}`",
                        function_name, port, owner
                    ),
                ));
            }

            out.entry(handler.clone()).or_default().insert(port.clone());

            for s in body {
                collect_yield_grants_stmt(
                    s,
                    function_name,
                    port_owners,
                    interrupt_names,
                    out,
                    source,
                )?;
            }
        }
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_yield_grants_stmt(
                    s,
                    function_name,
                    port_owners,
                    interrupt_names,
                    out,
                    source,
                )?;
            }
            for s in else_body {
                collect_yield_grants_stmt(
                    s,
                    function_name,
                    port_owners,
                    interrupt_names,
                    out,
                    source,
                )?;
            }
        }
        Stmt::While { body, .. } | Stmt::Repeat { body, .. } | Stmt::CycleContract { body, .. } => {
            for s in body {
                collect_yield_grants_stmt(
                    s,
                    function_name,
                    port_owners,
                    interrupt_names,
                    out,
                    source,
                )?;
            }
        }
        Stmt::For { body, .. } => {
            for s in body {
                collect_yield_grants_stmt(
                    s,
                    function_name,
                    port_owners,
                    interrupt_names,
                    out,
                    source,
                )?;
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

fn source_line_for(source: &str, line: usize) -> Option<String> {
    if line == 0 {
        return None;
    }
    source.lines().nth(line.saturating_sub(1)).map(|v| v.to_string())
}
