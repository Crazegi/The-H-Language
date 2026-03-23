use hl_lexer::ast::{Expr, Program, Stmt};

#[derive(Debug, Clone)]
pub struct LintIssue {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

const ALLOWED_IMPORTS: &[&str] = &[
    "math",
    "logic",
    "hardware",
    "gpio",
    "uart",
    "spi",
    "i2c",
    "timer",
    "watchdog",
    "dma",
];

const FORBIDDEN_FLAT_BUILTINS: &[&str] = &[
    "input",
    "read_text",
    "write_text",
    "append_text",
    "exists",
    "delete_file",
    "env",
    "now_ms",
    "rand_int",
    "window_loop",
    "menu",
    "http_get",
    "json_parse",
    "script_args_count",
    "script_arg",
    "script_cwd",
    "script_chdir",
    "script_path_join",
    "script_dirname",
    "script_basename",
    "script_run",
    "script_run_capture",
    "script_run_capture_lines",
    "script_pipe",
    "script_exists",
    "script_mkdir",
    "script_mkdir_all",
    "script_list_dir",
    "script_copy",
    "script_move",
    "script_delete",
    "script_is_file",
    "script_is_dir",
    "script_env_set",
    "script_exit",
];

pub fn validate_embedded_profile(program: &Program) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    for (idx, module) in program.imports.iter().enumerate() {
        if !ALLOWED_IMPORTS.contains(&module.as_str()) {
            let span = program
                .import_spans
                .get(idx)
                .copied()
                .unwrap_or(hl_lexer::Span { line: 1, column: 1 });
            issues.push(LintIssue {
                line: span.line,
                column: span.column,
                message: format!(
                    "embedded profile forbids import `{}`; allowed imports: {}",
                    module,
                    ALLOWED_IMPORTS.join(", ")
                ),
            });
        }
    }

    for function in &program.functions {
        for stmt in &function.body {
            visit_stmt(stmt, &mut issues);
        }
    }

    issues
}

fn visit_stmt(stmt: &Stmt, issues: &mut Vec<LintIssue>) {
    match stmt {
        Stmt::ConstDecl { expr, .. } => visit_expr(expr, issues),
        Stmt::OwnDecl { expr, .. } => visit_expr(expr, issues),
        Stmt::RefDecl { .. } | Stmt::PortOwn { .. } | Stmt::PortRef { .. } => {}
        Stmt::YieldPort { body, .. } => {
            for nested in body {
                visit_stmt(nested, issues);
            }
        }
        Stmt::Assign { expr, .. } => visit_expr(expr, issues),
        Stmt::Instruction { rhs, .. } => visit_expr(rhs, issues),
        Stmt::If {
            condition,
            then_body,
            else_body,
            ..
        } => {
            visit_expr(condition, issues);
            for nested in then_body {
                visit_stmt(nested, issues);
            }
            for nested in else_body {
                visit_stmt(nested, issues);
            }
        }
        Stmt::While {
            condition, body, ..
        } => {
            visit_expr(condition, issues);
            for nested in body {
                visit_stmt(nested, issues);
            }
        }
        Stmt::Repeat { times, body, .. } => {
            visit_expr(times, issues);
            for nested in body {
                visit_stmt(nested, issues);
            }
        }
        Stmt::For { iterable, body, .. } => {
            visit_expr(iterable, issues);
            for nested in body {
                visit_stmt(nested, issues);
            }
        }
        Stmt::CycleContract { body, .. } => {
            for nested in body {
                visit_stmt(nested, issues);
            }
        }
        Stmt::PrintBlock { fields, .. } => {
            for (_, expr) in fields {
                visit_expr(expr, issues);
            }
        }
        Stmt::Return { expr, .. } => {
            if let Some(expr) = expr {
                visit_expr(expr, issues);
            }
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
        Stmt::Expr { expr, .. } => visit_expr(expr, issues),
    }
}

fn visit_expr(expr: &Expr, issues: &mut Vec<LintIssue>) {
    match expr {
        Expr::Number(_, _) | Expr::String(_, _) | Expr::Bool(_, _) | Expr::Maybe(_) | Expr::Var(_, _) => {}
        Expr::Unary { rhs, .. } => visit_expr(rhs, issues),
        Expr::Binary { left, right, .. } => {
            visit_expr(left, issues);
            visit_expr(right, issues);
        }
        Expr::Call { name, args, span } => {
            if is_forbidden_call(name) {
                issues.push(LintIssue {
                    line: span.line,
                    column: span.column,
                    message: format!("embedded profile forbids call `{}`", name),
                });
            }
            for arg in args {
                visit_expr(arg, issues);
            }
        }
        Expr::FieldAccess { base, .. } => visit_expr(base, issues),
    }
}

fn is_forbidden_call(name: &str) -> bool {
    if name.starts_with("desktop.") || name.starts_with("script.") {
        return true;
    }
    FORBIDDEN_FLAT_BUILTINS.contains(&name)
}
