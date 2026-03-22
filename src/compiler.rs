use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownCycleCostPolicy {
    Strict,
    Conservative,
}

impl UnknownCycleCostPolicy {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "strict" => Some(Self::Strict),
            "conservative" => Some(Self::Conservative),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CycleCostProfile {
    pub name: String,
    pub costs: HashMap<String, u64>,
    pub metadata: HashMap<String, CycleCostMetadata>,
    pub unknown_policy: UnknownCycleCostPolicy,
    pub conservative_fallback: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CycleCostMetadata {
    pub source: Option<String>,
    pub confidence: Option<String>,
    pub worst_case_cycles: Option<u64>,
}

impl CycleCostProfile {
    fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

#[derive(Debug, Clone)]
pub struct CycleProfileLoadError {
    pub message: String,
}

impl CycleProfileLoadError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CycleProfileLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CycleProfileLoadError {}

#[derive(Debug, Clone)]
struct RawProfileDef {
    extends: Option<String>,
    unknown_policy: Option<UnknownCycleCostPolicy>,
    conservative_fallback: Option<u64>,
    costs: HashMap<String, u64>,
    energy_nj_costs: HashMap<String, u64>,
    sources: HashMap<String, String>,
    confidence: HashMap<String, String>,
    worst_case_cycles: HashMap<String, u64>,
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
    pub cycle_profile_override: Option<CycleCostProfile>,
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
            cycle_profile_override: None,
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
    pub cycle_profile: String,
    pub declared_cycles: u64,
    pub measured_cycles: u64,
    pub declared_energy_nj: Option<u64>,
    pub measured_energy_nj: Option<u64>,
    pub padded_nops: u64,
    pub final_cycles: u64,
    pub on_underflow: ContractPolicy,
    pub on_overflow: ContractPolicy,
}

#[derive(Debug, Clone, Copy)]
struct ExecuteCost {
    cycles: u64,
    energy_nj: u64,
}

#[derive(Debug, Clone)]
pub struct ProfileDoctorEntry {
    pub key: String,
    pub status: String,
    pub cost: Option<u64>,
    pub source: Option<String>,
    pub confidence: Option<String>,
    pub worst_case_cycles: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProfileDoctorReport {
    pub profile_name: String,
    pub unknown_policy: UnknownCycleCostPolicy,
    pub conservative_fallback: u64,
    pub required_keys: Vec<String>,
    pub missing_keys: Vec<String>,
    pub entries: Vec<ProfileDoctorEntry>,
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

pub fn load_cycle_profiles_from_file(
    path: impl AsRef<Path>,
) -> Result<HashMap<String, CycleCostProfile>, CycleProfileLoadError> {
    let content = fs::read_to_string(path.as_ref()).map_err(|err| {
        CycleProfileLoadError::new(format!(
            "Failed to read cycle profile file {}: {}",
            path.as_ref().display(),
            err
        ))
    })?;
    load_cycle_profiles_from_toml_str(&content)
}

pub fn load_cycle_profiles_from_toml_str(
    input: &str,
) -> Result<HashMap<String, CycleCostProfile>, CycleProfileLoadError> {
    let value: toml::Value = input
        .parse()
        .map_err(|err| CycleProfileLoadError::new(format!("Invalid TOML in cycle profile file: {}", err)))?;

    let mut raw_defs: HashMap<String, RawProfileDef> = HashMap::new();
    let profiles_table = value
        .get("profiles")
        .and_then(|v| v.as_table())
        .ok_or_else(|| {
            CycleProfileLoadError::new("Cycle profile file must define [profiles.<name>] tables")
        })?;

    for (name, raw) in profiles_table {
        let table = raw.as_table().ok_or_else(|| {
            CycleProfileLoadError::new(format!("profiles.{} must be a TOML table", name))
        })?;

        let extends = table
            .get("extends")
            .map(|v| {
                v.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    CycleProfileLoadError::new(format!(
                        "profiles.{}.extends must be a string",
                        name
                    ))
                })
            })
            .transpose()?;

        let unknown_policy = table
            .get("unknown_policy")
            .map(|v| {
                let raw = v.as_str().ok_or_else(|| {
                    CycleProfileLoadError::new(format!(
                        "profiles.{}.unknown_policy must be a string",
                        name
                    ))
                })?;
                UnknownCycleCostPolicy::from_str(raw).ok_or_else(|| {
                    CycleProfileLoadError::new(format!(
                        "profiles.{}.unknown_policy must be strict or conservative",
                        name
                    ))
                })
            })
            .transpose()?;

        let conservative_fallback = table
            .get("conservative_fallback")
            .map(|v| {
                v.as_integer()
                    .and_then(|n| if n >= 0 { Some(n as u64) } else { None })
                    .ok_or_else(|| {
                        CycleProfileLoadError::new(format!(
                            "profiles.{}.conservative_fallback must be a non-negative integer",
                            name
                        ))
                    })
            })
            .transpose()?;

        let mut costs = HashMap::new();
        if let Some(costs_table) = table.get("costs") {
            let costs_table = costs_table.as_table().ok_or_else(|| {
                CycleProfileLoadError::new(format!("profiles.{}.costs must be a table", name))
            })?;
            collect_cost_entries(name, "", costs_table, &mut costs)?;
        }

        let mut energy_nj_costs = HashMap::new();
        if let Some(energy_table) = table.get("energy_nj") {
            let energy_table = energy_table.as_table().ok_or_else(|| {
                CycleProfileLoadError::new(format!("profiles.{}.energy_nj must be a table", name))
            })?;
            collect_cost_entries(name, "", energy_table, &mut energy_nj_costs)?;
        }

        let mut sources = HashMap::new();
        if let Some(sources_table) = table.get("sources") {
            let sources_table = sources_table.as_table().ok_or_else(|| {
                CycleProfileLoadError::new(format!("profiles.{}.sources must be a table", name))
            })?;
            collect_string_entries(name, "sources", "", sources_table, &mut sources)?;
        }

        let mut confidence = HashMap::new();
        if let Some(confidence_table) = table.get("confidence") {
            let confidence_table = confidence_table.as_table().ok_or_else(|| {
                CycleProfileLoadError::new(format!("profiles.{}.confidence must be a table", name))
            })?;
            collect_string_entries(
                name,
                "confidence",
                "",
                confidence_table,
                &mut confidence,
            )?;
        }

        let mut worst_case_cycles = HashMap::new();
        if let Some(worst_case_table) = table.get("worst_case_cycles") {
            let worst_case_table = worst_case_table.as_table().ok_or_else(|| {
                CycleProfileLoadError::new(format!(
                    "profiles.{}.worst_case_cycles must be a table",
                    name
                ))
            })?;
            collect_u64_entries(
                name,
                "worst_case_cycles",
                "",
                worst_case_table,
                &mut worst_case_cycles,
            )?;
        }

        raw_defs.insert(
            name.clone(),
            RawProfileDef {
                extends,
                unknown_policy,
                conservative_fallback,
                costs,
                energy_nj_costs,
                sources,
                confidence,
                worst_case_cycles,
            },
        );
    }

    let mut resolved = built_in_cycle_profiles();
    let names: Vec<String> = raw_defs.keys().cloned().collect();
    let mut stack = HashSet::new();
    for name in names {
        resolve_custom_profile(&name, &raw_defs, &mut resolved, &mut stack)?;
    }

    Ok(resolved)
}

fn collect_cost_entries(
    profile_name: &str,
    prefix: &str,
    table: &toml::map::Map<String, toml::Value>,
    out: &mut HashMap<String, u64>,
) -> Result<(), CycleProfileLoadError> {
    for (key, value) in table {
        let full_key = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}.{}", prefix, key)
        };

        if let Some(cost) = value
            .as_integer()
            .and_then(|n| if n >= 0 { Some(n as u64) } else { None })
        {
            out.insert(full_key, cost);
            continue;
        }

        if let Some(child) = value.as_table() {
            collect_cost_entries(profile_name, &full_key, child, out)?;
            continue;
        }

        return Err(CycleProfileLoadError::new(format!(
            "profiles.{}.costs.{} must be a non-negative integer",
            profile_name, full_key
        )));
    }
    Ok(())
}

fn collect_string_entries(
    profile_name: &str,
    table_name: &str,
    prefix: &str,
    table: &toml::map::Map<String, toml::Value>,
    out: &mut HashMap<String, String>,
) -> Result<(), CycleProfileLoadError> {
    for (key, value) in table {
        let full_key = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}.{}", prefix, key)
        };

        if let Some(v) = value.as_str() {
            out.insert(full_key, v.to_string());
            continue;
        }

        if let Some(child) = value.as_table() {
            collect_string_entries(profile_name, table_name, &full_key, child, out)?;
            continue;
        }

        return Err(CycleProfileLoadError::new(format!(
            "profiles.{}.{}.{} must be a string",
            profile_name, table_name, full_key
        )));
    }
    Ok(())
}

fn collect_u64_entries(
    profile_name: &str,
    table_name: &str,
    prefix: &str,
    table: &toml::map::Map<String, toml::Value>,
    out: &mut HashMap<String, u64>,
) -> Result<(), CycleProfileLoadError> {
    for (key, value) in table {
        let full_key = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}.{}", prefix, key)
        };

        if let Some(v) = value
            .as_integer()
            .and_then(|n| if n >= 0 { Some(n as u64) } else { None })
        {
            out.insert(full_key, v);
            continue;
        }

        if let Some(child) = value.as_table() {
            collect_u64_entries(profile_name, table_name, &full_key, child, out)?;
            continue;
        }

        return Err(CycleProfileLoadError::new(format!(
            "profiles.{}.{}.{} must be a non-negative integer",
            profile_name, table_name, full_key
        )));
    }
    Ok(())
}

fn built_in_cycle_profiles() -> HashMap<String, CycleCostProfile> {
    let mut out = HashMap::new();

    out.insert(
        "generic".to_string(),
        builtin_profile_for(CycleProfile::Generic).with_name("generic"),
    );
    out.insert(
        "avr-like".to_string(),
        builtin_profile_for(CycleProfile::AvrLike).with_name("avr-like"),
    );
    out.insert(
        "cortex-m0-like".to_string(),
        builtin_profile_for(CycleProfile::CortexM0Like).with_name("cortex-m0-like"),
    );

    out
}

fn resolve_custom_profile(
    name: &str,
    raw_defs: &HashMap<String, RawProfileDef>,
    resolved: &mut HashMap<String, CycleCostProfile>,
    stack: &mut HashSet<String>,
) -> Result<CycleCostProfile, CycleProfileLoadError> {
    if let Some(profile) = resolved.get(name) {
        return Ok(profile.clone());
    }

    let Some(def) = raw_defs.get(name) else {
        return Err(CycleProfileLoadError::new(format!(
            "Unknown parent cycle profile `{}`",
            name
        )));
    };

    if !stack.insert(name.to_string()) {
        return Err(CycleProfileLoadError::new(format!(
            "Cycle profile inheritance cycle detected at `{}`",
            name
        )));
    }

    let mut base = if let Some(parent) = &def.extends {
        resolve_custom_profile(parent, raw_defs, resolved, stack)?
    } else {
        CycleCostProfile {
            name: name.to_string(),
            costs: HashMap::new(),
            metadata: HashMap::new(),
            unknown_policy: UnknownCycleCostPolicy::Strict,
            conservative_fallback: 1,
        }
    };

    for (k, v) in &def.costs {
        base.costs.insert(k.clone(), *v);
    }
    for (k, v) in &def.energy_nj_costs {
        base.costs.insert(format!("energy.{}", k), *v);
    }
    for (k, v) in &def.sources {
        base.metadata
            .entry(k.clone())
            .or_default()
            .source = Some(v.clone());
    }
    for (k, v) in &def.confidence {
        base.metadata
            .entry(k.clone())
            .or_default()
            .confidence = Some(v.clone());
    }
    for (k, v) in &def.worst_case_cycles {
        base.metadata
            .entry(k.clone())
            .or_default()
            .worst_case_cycles = Some(*v);
    }
    if let Some(policy) = def.unknown_policy {
        base.unknown_policy = policy;
    }
    if let Some(fallback) = def.conservative_fallback {
        base.conservative_fallback = fallback;
    }
    base.name = name.to_string();

    stack.remove(name);
    resolved.insert(name.to_string(), base.clone());
    Ok(base)
}

fn builtin_profile_for(profile: CycleProfile) -> CycleCostProfile {
    let mut costs = HashMap::new();

    costs.insert("instr.mov".to_string(), 1);
    costs.insert("instr.add".to_string(), 1);
    costs.insert("instr.sub".to_string(), 1);
    costs.insert("instr.cmp".to_string(), 1);
    costs.insert(
        "instr.mul".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 3,
            CycleProfile::CortexM0Like => 1,
        },
    );
    costs.insert(
        "instr.div".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 4,
            CycleProfile::CortexM0Like => 8,
        },
    );
    costs.insert(
        "instr.mod".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 4,
            CycleProfile::CortexM0Like => 8,
        },
    );

    costs.insert("expr.atom".to_string(), 1);
    costs.insert("expr.unary".to_string(), 1);
    costs.insert("expr.binary.default".to_string(), 1);
    costs.insert(
        "expr.mul".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 3,
            CycleProfile::CortexM0Like => 1,
        },
    );
    costs.insert(
        "expr.div".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 4,
            CycleProfile::CortexM0Like => 8,
        },
    );
    costs.insert(
        "expr.mod".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 4,
            CycleProfile::CortexM0Like => 8,
        },
    );
    costs.insert("stmt.store".to_string(), 1);

    costs.insert("energy.instr.mov".to_string(), 1);
    costs.insert("energy.instr.add".to_string(), 1);
    costs.insert("energy.instr.sub".to_string(), 1);
    costs.insert("energy.instr.cmp".to_string(), 1);
    costs.insert(
        "energy.instr.mul".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 3,
            CycleProfile::CortexM0Like => 1,
        },
    );
    costs.insert(
        "energy.instr.div".to_string(),
        match profile {
            CycleProfile::Generic => 3,
            CycleProfile::AvrLike => 5,
            CycleProfile::CortexM0Like => 9,
        },
    );
    costs.insert(
        "energy.instr.mod".to_string(),
        match profile {
            CycleProfile::Generic => 3,
            CycleProfile::AvrLike => 5,
            CycleProfile::CortexM0Like => 9,
        },
    );
    costs.insert("energy.expr.atom".to_string(), 1);
    costs.insert("energy.expr.unary".to_string(), 1);
    costs.insert("energy.expr.binary.default".to_string(), 1);
    costs.insert(
        "energy.expr.mul".to_string(),
        match profile {
            CycleProfile::Generic => 2,
            CycleProfile::AvrLike => 3,
            CycleProfile::CortexM0Like => 1,
        },
    );
    costs.insert(
        "energy.expr.div".to_string(),
        match profile {
            CycleProfile::Generic => 3,
            CycleProfile::AvrLike => 5,
            CycleProfile::CortexM0Like => 9,
        },
    );
    costs.insert(
        "energy.expr.mod".to_string(),
        match profile {
            CycleProfile::Generic => 3,
            CycleProfile::AvrLike => 5,
            CycleProfile::CortexM0Like => 9,
        },
    );
    costs.insert("energy.stmt.store".to_string(), 1);

    CycleCostProfile {
        name: profile.as_str().to_string(),
        costs,
        metadata: HashMap::new(),
        unknown_policy: UnknownCycleCostPolicy::Strict,
        conservative_fallback: 1,
    }
}

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

pub fn diagnose_cycle_profile_coverage(
    program: &Program,
    options: &CompileOptions,
) -> ProfileDoctorReport {
    let active = options
        .cycle_profile_override
        .clone()
        .unwrap_or_else(|| builtin_profile_for(options.cycle_profile));

    let mut required = std::collections::BTreeSet::new();
    for function in &program.functions {
        for stmt in &function.body {
            collect_required_cycle_keys_stmt(stmt, &mut required);
        }
    }

    let required_keys: Vec<String> = required.into_iter().collect();
    let mut missing_keys = Vec::new();
    let mut entries = Vec::new();

    for key in &required_keys {
        let cost = active.costs.get(key).copied();
        let meta = active.metadata.get(key);
        let status = if cost.is_some() {
            "known".to_string()
        } else {
            missing_keys.push(key.clone());
            match active.unknown_policy {
                UnknownCycleCostPolicy::Strict => "missing_strict_error".to_string(),
                UnknownCycleCostPolicy::Conservative => "missing_conservative_fallback".to_string(),
            }
        };

        entries.push(ProfileDoctorEntry {
            key: key.clone(),
            status,
            cost,
            source: meta.and_then(|m| m.source.clone()),
            confidence: meta.and_then(|m| m.confidence.clone()),
            worst_case_cycles: meta.and_then(|m| m.worst_case_cycles),
        });
    }

    ProfileDoctorReport {
        profile_name: active.name,
        unknown_policy: active.unknown_policy,
        conservative_fallback: active.conservative_fallback,
        required_keys,
        missing_keys,
        entries,
    }
}

pub fn render_profile_doctor_report_text(report: &ProfileDoctorReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "profile={} unknown_policy={:?} conservative_fallback={} required_keys={} missing_keys={}\n",
        report.profile_name,
        report.unknown_policy,
        report.conservative_fallback,
        report.required_keys.len(),
        report.missing_keys.len()
    ));

    for entry in &report.entries {
        out.push_str(&format!(
            "key={} status={} cost={} confidence={} worst_case={} source={}\n",
            entry.key,
            entry.status,
            entry
                .cost
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            entry
                .confidence
                .as_deref()
                .unwrap_or("n/a"),
            entry
                .worst_case_cycles
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            entry.source.as_deref().unwrap_or("n/a")
        ));
    }

    out
}

fn collect_required_cycle_keys_stmt(
    stmt: &Stmt,
    keys: &mut std::collections::BTreeSet<String>,
) {
    match stmt {
        Stmt::CycleContract { body, .. } => {
            for stmt in body {
                collect_execute_required_cycle_keys(stmt, keys);
            }
        }
        Stmt::If {
            then_body,
            else_body,
            ..
        } => {
            for stmt in then_body {
                collect_required_cycle_keys_stmt(stmt, keys);
            }
            for stmt in else_body {
                collect_required_cycle_keys_stmt(stmt, keys);
            }
        }
        Stmt::While { body, .. } | Stmt::Repeat { body, .. } => {
            for stmt in body {
                collect_required_cycle_keys_stmt(stmt, keys);
            }
        }
        _ => {}
    }
}

fn collect_execute_required_cycle_keys(
    stmt: &Stmt,
    keys: &mut std::collections::BTreeSet<String>,
) {
    match stmt {
        Stmt::Instruction { op, .. } => {
            let key = match op {
                AstInstruction::Mov => "instr.mov",
                AstInstruction::Add => "instr.add",
                AstInstruction::Sub => "instr.sub",
                AstInstruction::Mul => "instr.mul",
                AstInstruction::Div => "instr.div",
                AstInstruction::Mod => "instr.mod",
                AstInstruction::Cmp => "instr.cmp",
            };
            keys.insert(key.to_string());
            keys.insert(format!("energy.{}", key));
        }
        Stmt::OwnDecl { expr, .. } | Stmt::Assign { expr, .. } => {
            collect_expr_cycle_keys(expr, keys);
            keys.insert("stmt.store".to_string());
            keys.insert("energy.stmt.store".to_string());
        }
        Stmt::If {
            condition,
            then_body,
            else_body,
        } => {
            collect_expr_cycle_keys(condition, keys);
            for stmt in then_body {
                collect_execute_required_cycle_keys(stmt, keys);
            }
            for stmt in else_body {
                collect_execute_required_cycle_keys(stmt, keys);
            }
        }
        Stmt::Repeat { times, body } => {
            collect_expr_cycle_keys(times, keys);
            for stmt in body {
                collect_execute_required_cycle_keys(stmt, keys);
            }
        }
        _ => {}
    }
}

fn collect_expr_cycle_keys(expr: &Expr, keys: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Maybe | Expr::Var(_) => {
            keys.insert("expr.atom".to_string());
            keys.insert("energy.expr.atom".to_string());
        }
        Expr::Unary { rhs, .. } => {
            collect_expr_cycle_keys(rhs, keys);
            keys.insert("expr.unary".to_string());
            keys.insert("energy.expr.unary".to_string());
        }
        Expr::Binary { left, op, right } => {
            collect_expr_cycle_keys(left, keys);
            collect_expr_cycle_keys(right, keys);
            let key = match op {
                BinaryOp::Mul => "expr.mul",
                BinaryOp::Div => "expr.div",
                BinaryOp::Mod => "expr.mod",
                _ => "expr.binary.default",
            };
            keys.insert(key.to_string());
            keys.insert(format!("energy.{}", key));
        }
        Expr::Call { .. } => {
            // Calls are currently rejected in execute blocks, but we still record
            // this baseline expression category for profile completeness diagnostics.
            keys.insert("expr.atom".to_string());
            keys.insert("energy.expr.atom".to_string());
        }
    }
}

struct FunctionCompiler {
    function_name: String,
    options: CompileOptions,
    active_cycle_profile: CycleCostProfile,
    code: Vec<Instruction>,
    temp_counter: usize,
    contract_counter: usize,
    contract_reports: Vec<ContractCompileReport>,
}

impl FunctionCompiler {
    fn new(function_name: String, options: CompileOptions) -> Self {
        let active_cycle_profile = options
            .cycle_profile_override
            .clone()
            .unwrap_or_else(|| builtin_profile_for(options.cycle_profile));

        Self {
            function_name,
            options,
            active_cycle_profile,
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
        let track_energy = spec.energy_nj.is_some();

        let mut total_cycles: u64 = 0;
        let mut total_energy_nj: u64 = 0;
        let mut const_env: HashMap<String, FoldValue> = HashMap::new();
        for s in body {
            let cost = self.compile_execute_stmt(s, &mut const_env, track_energy)?;
            total_cycles = total_cycles
                .checked_add(cost.cycles)
                .ok_or_else(|| CompileError::new("Cycle count overflow in contract block"))?;
            total_energy_nj = total_energy_nj
                .checked_add(cost.energy_nj)
                .ok_or_else(|| CompileError::new("Energy count overflow in contract block"))?;
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
                        self.active_cycle_profile.name,
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
                        self.active_cycle_profile.name,
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

        if let Some(energy_budget_nj) = spec.energy_nj {
            if total_energy_nj > energy_budget_nj {
                match spec.on_overflow {
                    ContractPolicy::CompileError => {
                        if self.options.strict_cycle_contracts {
                            return Err(CompileError::new(format!(
                                "Energy contract overflow in function `{}` contract #{} (profile: {}): block costs {} nJ but contract allows {} nJ",
                                self.function_name,
                                contract_index,
                                self.active_cycle_profile.name,
                                total_energy_nj,
                                energy_budget_nj
                            )));
                        }
                    }
                    ContractPolicy::PadNop => {
                        // Energy overflow cannot be solved by padding.
                    }
                }
            }
        }

        let final_cycles = total_cycles + padded_nops;
        self.contract_reports.push(ContractCompileReport {
            function_name: self.function_name.clone(),
            contract_index,
            cycle_profile: self.active_cycle_profile.name.clone(),
            declared_cycles: spec.cycles,
            measured_cycles: total_cycles,
            declared_energy_nj: spec.energy_nj,
            measured_energy_nj: if track_energy {
                Some(total_energy_nj)
            } else {
                None
            },
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
        track_energy: bool,
    ) -> Result<ExecuteCost, CompileError> {
        match stmt {
            Stmt::Instruction { op, target, rhs } => {
                let cycles = stmt_cycle_cost(stmt, &self.active_cycle_profile)?;
                let energy_nj = if track_energy {
                    stmt_energy_cost(stmt, &self.active_cycle_profile)?
                } else {
                    0
                };
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

                Ok(ExecuteCost { cycles, energy_nj })
            }
            Stmt::OwnDecl { name, expr } => {
                self.compile_stmt(stmt)?;
                let mut cycles = expr_cycle_cost(expr, &self.active_cycle_profile)?;
                cycles = cycles
                    .checked_add(cycle_cost(&self.active_cycle_profile, "stmt.store")?)
                    .ok_or_else(|| CompileError::new("Cycle count overflow in execute block"))?;

                let mut energy_nj = if track_energy {
                    expr_energy_cost(expr, &self.active_cycle_profile)?
                } else {
                    0
                };
                if track_energy {
                    energy_nj = energy_nj
                        .checked_add(cycle_cost(&self.active_cycle_profile, "energy.stmt.store")?)
                        .ok_or_else(|| {
                            CompileError::new("Energy count overflow in execute block")
                        })?;
                }

                if let Some(value) =
                    eval_execute_const_expr(expr, const_env, self.options.fast_math)?
                {
                    const_env.insert(name.clone(), value);
                } else {
                    const_env.remove(name);
                }
                Ok(ExecuteCost { cycles, energy_nj })
            }
            Stmt::Assign { name, expr } => {
                self.compile_stmt(stmt)?;
                let mut cycles = expr_cycle_cost(expr, &self.active_cycle_profile)?;
                cycles = cycles
                    .checked_add(cycle_cost(&self.active_cycle_profile, "stmt.store")?)
                    .ok_or_else(|| CompileError::new("Cycle count overflow in execute block"))?;

                let mut energy_nj = if track_energy {
                    expr_energy_cost(expr, &self.active_cycle_profile)?
                } else {
                    0
                };
                if track_energy {
                    energy_nj = energy_nj
                        .checked_add(cycle_cost(&self.active_cycle_profile, "energy.stmt.store")?)
                        .ok_or_else(|| {
                            CompileError::new("Energy count overflow in execute block")
                        })?;
                }

                if let Some(value) =
                    eval_execute_const_expr(expr, const_env, self.options.fast_math)?
                {
                    const_env.insert(name.clone(), value);
                } else {
                    const_env.remove(name);
                }
                Ok(ExecuteCost { cycles, energy_nj })
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

                let mut total = ExecuteCost {
                    cycles: 0,
                    energy_nj: 0,
                };
                for s in chosen {
                    let cost = self.compile_execute_stmt(s, const_env, track_energy)?;
                    total.cycles = total
                        .cycles
                        .checked_add(cost.cycles)
                        .ok_or_else(|| CompileError::new("Cycle count overflow in execute block"))?;
                    total.energy_nj = total
                        .energy_nj
                        .checked_add(cost.energy_nj)
                        .ok_or_else(|| {
                            CompileError::new("Energy count overflow in execute block")
                        })?;
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
                let mut total = ExecuteCost {
                    cycles: 0,
                    energy_nj: 0,
                };
                for _ in 0..reps {
                    for s in body {
                        let cost = self.compile_execute_stmt(s, const_env, track_energy)?;
                        total.cycles = total
                            .cycles
                            .checked_add(cost.cycles)
                            .ok_or_else(|| {
                                CompileError::new("Cycle count overflow in execute block")
                            })?;
                        total.energy_nj = total
                            .energy_nj
                            .checked_add(cost.energy_nj)
                            .ok_or_else(|| {
                                CompileError::new("Energy count overflow in execute block")
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

fn cycle_cost(profile: &CycleCostProfile, key: &str) -> Result<u64, CompileError> {
    if let Some(cost) = profile.costs.get(key) {
        return Ok(*cost);
    }

    match profile.unknown_policy {
        UnknownCycleCostPolicy::Strict => Err(CompileError::new(format!(
            "Unknown cycle cost key `{}` for profile `{}`",
            key, profile.name
        ))),
        UnknownCycleCostPolicy::Conservative => Ok(profile.conservative_fallback),
    }
}

fn expr_cycle_cost(expr: &Expr, profile: &CycleCostProfile) -> Result<u64, CompileError> {
    match expr {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Maybe | Expr::Var(_) => {
            cycle_cost(profile, "expr.atom")
        }
        Expr::Unary { rhs, .. } => {
            let rhs_cost = expr_cycle_cost(rhs, profile)?;
            rhs_cost
                .checked_add(cycle_cost(profile, "expr.unary")?)
                .ok_or_else(|| CompileError::new("Cycle count overflow in expression"))
        }
        Expr::Binary { left, op, right } => {
            let left_cost = expr_cycle_cost(left, profile)?;
            let right_cost = expr_cycle_cost(right, profile)?;
            let op_cost = match op {
                BinaryOp::Mul => cycle_cost(profile, "expr.mul")?,
                BinaryOp::Div => cycle_cost(profile, "expr.div")?,
                BinaryOp::Mod => cycle_cost(profile, "expr.mod")?,
                _ => cycle_cost(profile, "expr.binary.default")?,
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

fn expr_energy_cost(expr: &Expr, profile: &CycleCostProfile) -> Result<u64, CompileError> {
    match expr {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Maybe | Expr::Var(_) => {
            cycle_cost(profile, "energy.expr.atom")
        }
        Expr::Unary { rhs, .. } => {
            let rhs_cost = expr_energy_cost(rhs, profile)?;
            rhs_cost
                .checked_add(cycle_cost(profile, "energy.expr.unary")?)
                .ok_or_else(|| CompileError::new("Energy count overflow in expression"))
        }
        Expr::Binary { left, op, right } => {
            let left_cost = expr_energy_cost(left, profile)?;
            let right_cost = expr_energy_cost(right, profile)?;
            let op_cost = match op {
                BinaryOp::Mul => cycle_cost(profile, "energy.expr.mul")?,
                BinaryOp::Div => cycle_cost(profile, "energy.expr.div")?,
                BinaryOp::Mod => cycle_cost(profile, "energy.expr.mod")?,
                _ => cycle_cost(profile, "energy.expr.binary.default")?,
            };

            left_cost
                .checked_add(right_cost)
                .and_then(|v| v.checked_add(op_cost))
                .ok_or_else(|| CompileError::new("Energy count overflow in expression"))
        }
        Expr::Call { .. } => Err(CompileError::new(
            "Cycle contract execute block does not allow function calls",
        )),
    }
}

fn stmt_cycle_cost(stmt: &Stmt, profile: &CycleCostProfile) -> Result<u64, CompileError> {
    match stmt {
        Stmt::Instruction { op, .. } => Ok(match op {
            AstInstruction::Mov => cycle_cost(profile, "instr.mov")?,
            AstInstruction::Add => cycle_cost(profile, "instr.add")?,
            AstInstruction::Sub => cycle_cost(profile, "instr.sub")?,
            AstInstruction::Cmp => cycle_cost(profile, "instr.cmp")?,
            AstInstruction::Mul => cycle_cost(profile, "instr.mul")?,
            AstInstruction::Div => cycle_cost(profile, "instr.div")?,
            AstInstruction::Mod => cycle_cost(profile, "instr.mod")?,
        }),
        _ => Err(CompileError::new(
            "Cycle contract execute block supports only assembly instructions",
        )),
    }
}

fn stmt_energy_cost(stmt: &Stmt, profile: &CycleCostProfile) -> Result<u64, CompileError> {
    match stmt {
        Stmt::Instruction { op, .. } => Ok(match op {
            AstInstruction::Mov => cycle_cost(profile, "energy.instr.mov")?,
            AstInstruction::Add => cycle_cost(profile, "energy.instr.add")?,
            AstInstruction::Sub => cycle_cost(profile, "energy.instr.sub")?,
            AstInstruction::Cmp => cycle_cost(profile, "energy.instr.cmp")?,
            AstInstruction::Mul => cycle_cost(profile, "energy.instr.mul")?,
            AstInstruction::Div => cycle_cost(profile, "energy.instr.div")?,
            AstInstruction::Mod => cycle_cost(profile, "energy.instr.mod")?,
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
            "function={} contract=#{} profile={} declared={} measured={} declared_energy_nj={} measured_energy_nj={} padded_nops={} final={} underflow={:?} overflow={:?}\n",
            report.function_name,
            report.contract_index,
            report.cycle_profile,
            report.declared_cycles,
            report.measured_cycles,
            report
                .declared_energy_nj
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            report
                .measured_energy_nj
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            report.padded_nops,
            report.final_cycles,
            report.on_underflow,
            report.on_overflow
        ));
    }
    out
}
