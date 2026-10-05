use crate::analyzer::upgrade::WasmType;
use crate::{DebuggerError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use wasmparser::{
    Name, NameSectionReader, Operator, Parser, Payload, ProducersSectionReader, ValType,
};

// Re-export FunctionSignature for convenience
pub use crate::analyzer::upgrade::FunctionSignature;
// ─── existing public API (unchanged) ─────────────────────────────────────────

// ─── arithmetic analysis (new) ────────────────────────────────────────────────

/// Decoded WASM instruction for arithmetic analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmInstruction {
    I32Add,
    I32Sub,
    I32Mul,
    I64Add,
    I64Sub,
    I64Mul,
    If,
    BrIf,
    Call,
    I32Const,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareKind {
    Eqz,
    Eq,
    Ne,
    LtS,
    LtU,
    GtS,
    GtU,
    LeS,
    LeU,
    GeS,
    GeU,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    If,
    BrIf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArithmeticConfidence {
    High,
    Medium,
    Low,
}

impl ArithmeticConfidence {
    pub fn label(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    pub fn score(&self) -> f32 {
        match self {
            Self::High => 0.95,
            Self::Medium => 0.70,
            Self::Low => 0.40,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArithmeticOpAnalysis {
    pub function_index: u32,
    pub instruction_index: usize,
    pub offset: usize,
    pub instruction: WasmInstruction,
    pub confidence: ArithmeticConfidence,
    pub rationale: String,
}

/// Decode a single WASM instruction byte to its instruction type.
fn decode_instruction(byte: u8) -> WasmInstruction {
    match byte {
        0x6A => WasmInstruction::I32Add,
        0x6B => WasmInstruction::I32Sub,
        0x6C => WasmInstruction::I32Mul,
        0x7C => WasmInstruction::I64Add,
        0x7D => WasmInstruction::I64Sub,
        0x7E => WasmInstruction::I64Mul,
        0x04 => WasmInstruction::If,
        0x0D => WasmInstruction::BrIf,
        0x10 => WasmInstruction::Call,
        0x41 => WasmInstruction::I32Const,
        other => WasmInstruction::Unknown(other),
    }
}

/// Parse WASM bytecode into a vector of instructions (single-pass linear scan).
pub fn parse_instructions(wasm: &[u8]) -> Vec<WasmInstruction> {
    wasm.iter().map(|b| decode_instruction(*b)).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StackValueKind {
    Unknown,
    Compare(CompareKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StackValue {
    arithmetic_dependencies: BTreeSet<usize>,
    kind: StackValueKind,
}

impl StackValue {
    fn unknown() -> Self {
        Self {
            arithmetic_dependencies: BTreeSet::new(),
            kind: StackValueKind::Unknown,
        }
    }

    fn from_arithmetic(arithmetic_index: usize) -> Self {
        let mut arithmetic_dependencies = BTreeSet::new();
        arithmetic_dependencies.insert(arithmetic_index);
        Self {
            arithmetic_dependencies,
            kind: StackValueKind::Unknown,
        }
    }

    fn merge(kind: StackValueKind, inputs: impl IntoIterator<Item = StackValue>) -> Self {
        let mut arithmetic_dependencies = BTreeSet::new();
        for value in inputs {
            arithmetic_dependencies.extend(value.arithmetic_dependencies);
        }
        Self {
            arithmetic_dependencies,
            kind,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ArithmeticObservations {
    compare_guards: Vec<(CompareKind, BranchKind)>,
    compares_without_branch: Vec<CompareKind>,
    direct_branches: Vec<BranchKind>,
}

pub fn analyze_arithmetic_ops(wasm: &[u8]) -> Result<Vec<ArithmeticOpAnalysis>> {
    let mut findings = Vec::new();
    let mut saw_code = false;
    let mut function_index = 0u32;

    for payload in Parser::new(0).parse_all(wasm) {
        let payload = match payload {
            Ok(payload) => payload,
            Err(_) => {
                return Ok(analyze_raw_arithmetic_ops(wasm));
            }
        };

        if let Payload::CodeSectionEntry(body) = payload {
            saw_code = true;
            findings.extend(analyze_function_arithmetic(body, function_index)?);
            function_index += 1;
        }
    }

    if saw_code {
        Ok(findings)
    } else {
        Ok(analyze_raw_arithmetic_ops(wasm))
    }
}

fn analyze_function_arithmetic(
    body: wasmparser::FunctionBody<'_>,
    function_index: u32,
) -> Result<Vec<ArithmeticOpAnalysis>> {
    let mut stack = Vec::<StackValue>::new();
    let mut locals = HashMap::<u32, StackValue>::new();
    let mut arithmetic_ops = Vec::<(usize, usize, WasmInstruction)>::new();
    let mut observations = Vec::<ArithmeticObservations>::new();

    let mut reader = body.get_operators_reader().map_err(|e| {
        DebuggerError::WasmLoadError(format!("Failed to read function operators: {}", e))
    })?;
    let mut instruction_index = 0usize;

    while !reader.eof() {
        let offset = reader.original_position();
        let op = reader
            .read()
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to read operator: {}", e)))?;

        match op {
            Operator::LocalGet { local_index } => {
                let value = locals
                    .get(&local_index)
                    .cloned()
                    .unwrap_or_else(StackValue::unknown);
                stack.push(value);
            }
            Operator::LocalSet { local_index } => {
                let value = stack.pop().unwrap_or_else(StackValue::unknown);
                locals.insert(local_index, value);
            }
            Operator::LocalTee { local_index } => {
                let value = stack.pop().unwrap_or_else(StackValue::unknown);
                locals.insert(local_index, value.clone());
                stack.push(value);
            }
            Operator::I32Const { .. } | Operator::I64Const { .. } => {
                stack.push(StackValue::unknown());
            }
            Operator::Drop => {
                let _ = stack.pop();
            }
            Operator::Select => {
                let _condition = stack.pop();
                let fallback = stack.pop().unwrap_or_else(StackValue::unknown);
                let primary = stack.pop().unwrap_or_else(StackValue::unknown);
                stack.push(StackValue::merge(
                    StackValueKind::Unknown,
                    [primary, fallback],
                ));
            }
            Operator::I32Add
            | Operator::I32Sub
            | Operator::I32Mul
            | Operator::I64Add
            | Operator::I64Sub
            | Operator::I64Mul => {
                let _rhs = stack.pop();
                let _lhs = stack.pop();
                let arithmetic_index = arithmetic_ops.len();
                let instruction = match op {
                    Operator::I32Add => WasmInstruction::I32Add,
                    Operator::I32Sub => WasmInstruction::I32Sub,
                    Operator::I32Mul => WasmInstruction::I32Mul,
                    Operator::I64Add => WasmInstruction::I64Add,
                    Operator::I64Sub => WasmInstruction::I64Sub,
                    Operator::I64Mul => WasmInstruction::I64Mul,
                    _ => unreachable!(),
                };
                arithmetic_ops.push((instruction_index, offset, instruction));
                observations.push(ArithmeticObservations::default());
                stack.push(StackValue::from_arithmetic(arithmetic_index));
            }
            Operator::I32Eqz | Operator::I64Eqz => {
                let value = stack.pop().unwrap_or_else(StackValue::unknown);
                note_compare(
                    &mut observations,
                    &value.arithmetic_dependencies,
                    CompareKind::Eqz,
                );
                stack.push(StackValue::merge(
                    StackValueKind::Compare(CompareKind::Eqz),
                    [value],
                ));
            }
            Operator::I32Eq
            | Operator::I32Ne
            | Operator::I32LtS
            | Operator::I32LtU
            | Operator::I32GtS
            | Operator::I32GtU
            | Operator::I32LeS
            | Operator::I32LeU
            | Operator::I32GeS
            | Operator::I32GeU
            | Operator::I64Eq
            | Operator::I64Ne
            | Operator::I64LtS
            | Operator::I64LtU
            | Operator::I64GtS
            | Operator::I64GtU
            | Operator::I64LeS
            | Operator::I64LeU
            | Operator::I64GeS
            | Operator::I64GeU => {
                let rhs = stack.pop().unwrap_or_else(StackValue::unknown);
                let lhs = stack.pop().unwrap_or_else(StackValue::unknown);
                let compare_kind = compare_kind(&op).expect("comparison operator expected");
                let deps = StackValue::merge(StackValueKind::Compare(compare_kind), [lhs, rhs]);
                note_compare(
                    &mut observations,
                    &deps.arithmetic_dependencies,
                    compare_kind,
                );
                stack.push(deps);
            }
            Operator::If { .. } => {
                let condition = stack.pop().unwrap_or_else(StackValue::unknown);
                note_branch(&mut observations, &condition, BranchKind::If);
            }
            Operator::BrIf { .. } => {
                let condition = stack.pop().unwrap_or_else(StackValue::unknown);
                note_branch(&mut observations, &condition, BranchKind::BrIf);
            }
            _ => {}
        }

        instruction_index += 1;
    }

    Ok(arithmetic_ops
        .into_iter()
        .enumerate()
        .filter_map(|(arith_index, (instruction_index, offset, instruction))| {
            classify_arithmetic_observation(
                function_index,
                instruction_index,
                offset,
                instruction,
                &observations[arith_index],
            )
        })
        .collect())
}

fn note_compare(
    observations: &mut [ArithmeticObservations],
    dependencies: &BTreeSet<usize>,
    compare_kind: CompareKind,
) {
    for dependency in dependencies {
        if let Some(observation) = observations.get_mut(*dependency) {
            observation.compares_without_branch.push(compare_kind);
        }
    }
}

fn note_branch(
    observations: &mut [ArithmeticObservations],
    condition: &StackValue,
    branch_kind: BranchKind,
) {
    for dependency in &condition.arithmetic_dependencies {
        if let Some(observation) = observations.get_mut(*dependency) {
            match condition.kind {
                StackValueKind::Compare(compare_kind) => {
                    observation.compare_guards.push((compare_kind, branch_kind));
                    if let Some(position) = observation
                        .compares_without_branch
                        .iter()
                        .position(|kind| *kind == compare_kind)
                    {
                        observation.compares_without_branch.remove(position);
                    }
                }
                StackValueKind::Unknown => observation.direct_branches.push(branch_kind),
            }
        }
    }
}

fn classify_arithmetic_observation(
    function_index: u32,
    instruction_index: usize,
    offset: usize,
    instruction: WasmInstruction,
    observation: &ArithmeticObservations,
) -> Option<ArithmeticOpAnalysis> {
    if !observation.compare_guards.is_empty() {
        return None;
    }

    let (confidence, rationale) = if !observation.direct_branches.is_empty() {
        (
            ArithmeticConfidence::Low,
            format!(
                "The arithmetic result influences {:?}, but no recognized compare-and-branch guard was observed.",
                observation.direct_branches
            ),
        )
    } else if !observation.compares_without_branch.is_empty() {
        (
            ArithmeticConfidence::Medium,
            format!(
                "The arithmetic result is compared via {:?}, but that comparison does not drive conditional control flow.",
                observation.compares_without_branch
            ),
        )
    } else {
        (
            ArithmeticConfidence::High,
            "No comparison-derived conditional branch was observed for the arithmetic result."
                .to_string(),
        )
    };

    Some(ArithmeticOpAnalysis {
        function_index,
        instruction_index,
        offset,
        instruction,
        confidence,
        rationale,
    })
}

fn compare_kind(op: &Operator<'_>) -> Option<CompareKind> {
    match op {
        Operator::I32Eqz | Operator::I64Eqz => Some(CompareKind::Eqz),
        Operator::I32Eq | Operator::I64Eq => Some(CompareKind::Eq),
        Operator::I32Ne | Operator::I64Ne => Some(CompareKind::Ne),
        Operator::I32LtS | Operator::I64LtS => Some(CompareKind::LtS),
        Operator::I32LtU | Operator::I64LtU => Some(CompareKind::LtU),
        Operator::I32GtS | Operator::I64GtS => Some(CompareKind::GtS),
        Operator::I32GtU | Operator::I64GtU => Some(CompareKind::GtU),
        Operator::I32LeS | Operator::I64LeS => Some(CompareKind::LeS),
        Operator::I32LeU | Operator::I64LeU => Some(CompareKind::LeU),
        Operator::I32GeS | Operator::I64GeS => Some(CompareKind::GeS),
        Operator::I32GeU | Operator::I64GeU => Some(CompareKind::GeU),
        _ => None,
    }
}

fn analyze_raw_arithmetic_ops(wasm: &[u8]) -> Vec<ArithmeticOpAnalysis> {
    parse_instructions(wasm)
        .into_iter()
        .enumerate()
        .filter(|(_, instruction)| {
            matches!(
                instruction,
                WasmInstruction::I32Add
                    | WasmInstruction::I32Sub
                    | WasmInstruction::I32Mul
                    | WasmInstruction::I64Add
                    | WasmInstruction::I64Sub
                    | WasmInstruction::I64Mul
            )
        })
        .map(|(instruction_index, instruction)| ArithmeticOpAnalysis {
            function_index: 0,
            instruction_index,
            offset: instruction_index,
            instruction,
            confidence: ArithmeticConfidence::High,
            rationale: "The input is not a structured WASM module, so no semantic guard analysis was possible.".to_string(),
        })
        .collect()
}

/// Compute the SHA-256 checksum of a WASM binary.
pub fn compute_checksum(wasm_bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(wasm_bytes);
    hex::encode(hasher.finalize())
}

/// Parse exported functions from a WASM module.
pub fn parse_functions(wasm_bytes: &[u8]) -> Result<Vec<String>> {
    let mut functions = Vec::new();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        if let Payload::ExportSection(reader) = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?
        {
            for export in reader {
                let export = export.map_err(|e| {
                    DebuggerError::WasmLoadError(format!("Failed to read export: {}", e))
                })?;
                if matches!(export.kind, wasmparser::ExternalKind::Func) {
                    functions.push(export.name.to_string());
                }
            }
        }
    }

    Ok(functions)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossContractCall {
    pub caller: String,
    pub target: String,
    pub host_function: String,
}

fn is_cross_contract_import(module: &str, name: &str) -> bool {
    let module = module.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();

    (module.contains("env") || module.contains("soroban"))
        && (name.contains("invoke_contract")
            || name.contains("call_contract")
            || name.contains("try_call"))
}

fn map_import_to_target(import_name: &str) -> String {
    let import_name = import_name.to_ascii_lowercase();
    if import_name.contains("invoke_contract") || import_name.contains("call_contract") {
        "external_contract".to_string()
    } else {
        format!("external::{}", import_name)
    }
}

/// Parse cross-contract call sites by scanning WASM calls to known host imports.
pub fn parse_cross_contract_calls(wasm_bytes: &[u8]) -> Result<Vec<CrossContractCall>> {
    use std::collections::{BTreeSet, HashMap};
    use wasmparser::Operator;

    let mut export_names: HashMap<u32, String> = HashMap::new();
    let mut cross_contract_imports: HashMap<u32, String> = HashMap::new();
    let mut imported_func_count = 0u32;
    let mut local_function_index = 0u32;
    let mut calls = Vec::new();
    let mut dedupe = BTreeSet::new();

    for payload in Parser::new(0).parse_all(wasm_bytes) {
        match payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?
        {
            Payload::ImportSection(reader) => {
                for import in reader {
                    let import = import.map_err(|e| {
                        DebuggerError::WasmLoadError(format!("Failed to read import: {}", e))
                    })?;
                    if let wasmparser::TypeRef::Func(_) = import.ty {
                        let current_index = imported_func_count;
                        imported_func_count += 1;
                        if is_cross_contract_import(import.module, import.name) {
                            cross_contract_imports.insert(current_index, import.name.to_string());
                        }
                    }
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(|e| {
                        DebuggerError::WasmLoadError(format!("Failed to read export: {}", e))
                    })?;
                    if matches!(export.kind, wasmparser::ExternalKind::Func) {
                        export_names.insert(export.index, export.name.to_string());
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let current_fn_index = imported_func_count + local_function_index;
                local_function_index += 1;
                let caller = export_names
                    .get(&current_fn_index)
                    .cloned()
                    .unwrap_or_else(|| format!("func_{current_fn_index}"));

                let mut reader = body.get_operators_reader().map_err(|e| {
                    DebuggerError::WasmLoadError(format!("Failed to get operators reader: {}", e))
                })?;
                while !reader.eof() {
                    if let Operator::Call { function_index } = reader.read().map_err(|e| {
                        DebuggerError::WasmLoadError(format!("Failed to read operator: {}", e))
                    })? {
                        if let Some(host_fn_name) = cross_contract_imports.get(&function_index) {
                            let target = map_import_to_target(host_fn_name);
                            let key = format!("{caller}->{target}:{host_fn_name}");
                            if dedupe.insert(key) {
                                calls.push(CrossContractCall {
                                    caller: caller.clone(),
                                    target,
                                    host_function: host_fn_name.clone(),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(calls)
}

pub fn get_module_info(wasm_bytes: &[u8]) -> Result<ModuleInfo> {
    let mut info = ModuleInfo {
        total_size: wasm_bytes.len(),
        ..ModuleInfo::default()
    };

    let mut add_section = |name: String, range: std::ops::Range<usize>| {
        info.sections.push(WasmSection {
            name,
            size: range.end - range.start,
            offset: range.start,
        });
    };

    for payload in Parser::new(0).parse_all(wasm_bytes) {
        let payload = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?;
        match payload {
            Payload::TypeSection(reader) => {
                info.type_count = reader.count();
                add_section("Type".into(), reader.range());
            }
            Payload::ImportSection(reader) => add_section("Import".into(), reader.range()),
            Payload::FunctionSection(reader) => {
                info.function_count = reader.count();
                add_section("Function".into(), reader.range());
            }
            Payload::TableSection(reader) => add_section("Table".into(), reader.range()),
            Payload::MemorySection(reader) => add_section("Memory".into(), reader.range()),
            Payload::GlobalSection(reader) => add_section("Global".into(), reader.range()),
            Payload::ExportSection(reader) => {
                info.export_count = reader.count();
                add_section("Export".into(), reader.range());
            }
            Payload::StartSection { range, .. } => add_section("Start".into(), range),
            Payload::ElementSection(reader) => add_section("Element".into(), reader.range()),
            Payload::CodeSectionStart { range, .. } => add_section("Code".into(), range),
            Payload::CodeSectionEntry(reader) => add_section("Code (Entry)".into(), reader.range()),
            Payload::DataSection(reader) => add_section("Data".into(), reader.range()),
            Payload::DataCountSection { range, .. } => add_section("Data Count".into(), range),
            Payload::CustomSection(reader) => {
                add_section(format!("Custom ({})", reader.name()), reader.range())
            }
            _ => {}
        }
    }

    Ok(info)
}

/// Returns the byte range of the WASM code section payload within the module, if present.
///
/// This range is suitable for normalizing DWARF line-program addresses that are expressed
/// as offsets into the code section.
pub fn code_section_range(wasm_bytes: &[u8]) -> Result<Option<std::ops::Range<usize>>> {
    for payload in Parser::new(0).parse_all(wasm_bytes) {
        let payload = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?;
        if let Payload::CodeSectionStart { range, .. } = payload {
            return Ok(Some(range));
        }
    }

    Ok(None)
}

/// Information about a WASM module.
#[derive(Debug, Default, Serialize)]
pub struct ModuleInfo {
    pub total_size: usize,
    pub type_count: u32,
    pub function_count: u32,
    pub export_count: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<WasmSection>,
}

/// Parse full function signatures (name + param types + return types) from WASM
/// Represents a single section within a WASM binary.
#[derive(Debug, Serialize, Clone)]
pub struct WasmSection {
    pub name: String,
    pub size: usize,
    pub offset: usize,
}

// ─── wasm loading & checksum ──────────────────────────────────────────────────

/// Holds the raw bytes and computed SHA-256 hash of a loaded WASM file.
#[derive(Debug, Clone)]
pub struct WasmFile {
    pub bytes: Vec<u8>,
    pub sha256_hash: String,
}

/// Computes the SHA-256 hash of the given WASM bytes.
/// Returns the hash as a lowercase hexadecimal string.
pub fn compute_wasm_sha256(wasm_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(wasm_bytes);
    hex::encode(hasher.finalize())
}

/// Reads a WASM file from disk and computes its SHA-256 checksum.
pub fn load_wasm<P: AsRef<Path>>(path: P) -> Result<WasmFile> {
    let path_ref = path.as_ref();
    let bytes = fs::read(path_ref).map_err(|e| {
        crate::DebuggerError::WasmLoadError(format!(
            "Failed to read WASM file at {:?}: {}",
            path_ref, e
        ))
    })?;
    let sha256_hash = compute_wasm_sha256(&bytes);
    Ok(WasmFile { bytes, sha256_hash })
}

/// Verifies that the computed hash matches the expected hash, if one is provided.
pub fn verify_wasm_hash(computed_hash: &str, expected_hash: Option<&String>) -> Result<()> {
    if let Some(expected) = expected_hash {
        if expected.to_lowercase() != computed_hash {
            return Err(crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                computed_hash.to_string(),
            )
            .into());
        }
    }
    Ok(())
}

// ─── metadata types ───────────────────────────────────────────────────────────

/// High-level contract metadata extracted from WASM custom sections.
///
/// All fields are optional; missing values are handled gracefully.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractMetadata {
    pub contract_version: Option<String>,
    pub sdk_version: Option<String>,
    pub build_date: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub implementation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmProducerValue {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmProducerField {
    pub name: String,
    pub values: Vec<WasmProducerValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmArtifactMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_name: Option<String>,
    pub name_section_present: bool,
    pub has_debug_sections: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub debug_sections: Vec<String>,
    pub debug_section_bytes: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub producers: Vec<WasmProducerField>,
    pub build_profile_hint: String,
    pub optimization_hint: String,
    pub likely_stripped: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub package_hints: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub heuristic_notes: Vec<String>,
}

impl Default for WasmArtifactMetadata {
    fn default() -> Self {
        Self {
            module_name: None,
            name_section_present: false,
            has_debug_sections: false,
            debug_sections: Vec::new(),
            debug_section_bytes: 0,
            producers: Vec::new(),
            build_profile_hint: "unknown".to_string(),
            optimization_hint: "unknown".to_string(),
            likely_stripped: false,
            package_hints: Vec::new(),
            heuristic_notes: Vec::new(),
        }
    }
}

fn is_debug_custom_section(name: &str) -> bool {
    name.starts_with(".debug_")
}

fn push_hint(hints: &mut Vec<String>, hint: impl Into<String>) {
    let hint = hint.into();
    if !hints.contains(&hint) {
        hints.push(hint);
    }
}

fn derive_artifact_heuristics(
    module_name: Option<&str>,
    name_section_present: bool,
    debug_sections: &[String],
    producers: &[WasmProducerField],
) -> (String, String, bool, Vec<String>, Vec<String>) {
    let has_debug_sections = !debug_sections.is_empty();
    let has_wasm_opt = producers.iter().any(|field| {
        field.name == "processed-by"
            && field
                .values
                .iter()
                .any(|value| value.name.eq_ignore_ascii_case("wasm-opt"))
    });
    let has_rust_tooling = producers.iter().any(|field| {
        field.values.iter().any(|value| {
            value.name.eq_ignore_ascii_case("rust")
                || value.name.eq_ignore_ascii_case("rustc")
                || value.name.eq_ignore_ascii_case("cargo")
        })
    });

    let build_profile_hint = if has_debug_sections && name_section_present {
        "debug-like"
    } else if has_debug_sections {
        "release-with-debug-info"
    } else if name_section_present {
        "release-with-symbol-names"
    } else {
        "stripped-release-like"
    }
    .to_string();

    let optimization_hint = if has_wasm_opt {
        "aggressively optimized (wasm-opt present)"
    } else if !has_debug_sections && !name_section_present {
        "likely optimized or symbol-stripped"
    } else if has_debug_sections && name_section_present {
        "unlikely to be aggressively optimized"
    } else if has_debug_sections {
        "possibly optimized, but debug info was retained"
    } else {
        "limited optimization signal"
    }
    .to_string();

    let likely_stripped = !has_debug_sections && !name_section_present;

    let mut package_hints = Vec::new();
    if let Some(module_name) = module_name {
        push_hint(&mut package_hints, format!("module name: {module_name}"));
    }
    if has_rust_tooling {
        push_hint(&mut package_hints, "rust toolchain markers present");
    }
    for field in producers {
        for value in &field.values {
            match field.name.as_str() {
                "language" => {
                    if value.version.is_empty() {
                        push_hint(&mut package_hints, format!("language: {}", value.name));
                    } else {
                        push_hint(
                            &mut package_hints,
                            format!("language: {} {}", value.name, value.version),
                        );
                    }
                }
                "sdk" => {
                    if value.version.is_empty() {
                        push_hint(&mut package_hints, format!("sdk: {}", value.name));
                    } else {
                        push_hint(
                            &mut package_hints,
                            format!("sdk: {} {}", value.name, value.version),
                        );
                    }
                }
                "processed-by"
                    if matches!(
                        value.name.as_str(),
                        "cargo" | "rustc" | "soroban-sdk" | "soroban-cli" | "wasm-opt"
                    ) =>
                {
                    if value.version.is_empty() {
                        push_hint(&mut package_hints, format!("tool: {}", value.name));
                    } else {
                        push_hint(
                            &mut package_hints,
                            format!("tool: {} {}", value.name, value.version),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    let mut heuristic_notes = Vec::new();
    if has_debug_sections {
        heuristic_notes.push(format!(
            "DWARF-style debug sections detected: {}.",
            debug_sections.join(", ")
        ));
    } else {
        heuristic_notes.push(
            "No DWARF-style debug sections were detected; source-level debugging may be limited."
                .to_string(),
        );
    }

    if name_section_present {
        heuristic_notes.push(
            "A WASM name section is present, so symbol names are likely retained.".to_string(),
        );
    } else {
        heuristic_notes.push(
            "No WASM name section was detected, which often indicates a more stripped artifact."
                .to_string(),
        );
    }

    if has_wasm_opt {
        heuristic_notes.push(
            "The producers section reports wasm-opt, which is a strong signal of post-build optimization."
                .to_string(),
        );
    }

    (
        build_profile_hint,
        optimization_hint,
        likely_stripped,
        package_hints,
        heuristic_notes,
    )
}

pub fn extract_wasm_artifact_metadata(wasm_bytes: &[u8]) -> Result<WasmArtifactMetadata> {
    let mut module_name = None;
    let mut name_section_present = false;
    let mut debug_sections = Vec::new();
    let mut debug_section_bytes = 0usize;
    let mut producers = Vec::new();

    for payload in Parser::new(0).parse_all(wasm_bytes) {
        let payload = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?;

        let Payload::CustomSection(reader) = payload else {
            continue;
        };

        let section_name = reader.name();
        if section_name == "name" {
            name_section_present = true;
            let name_reader = NameSectionReader::new(reader.data(), reader.data_offset());
            for subsection in name_reader {
                let subsection: Name<'_> = subsection.map_err(|e| {
                    DebuggerError::WasmLoadError(format!(
                        "Failed to read WASM name subsection: {}",
                        e
                    ))
                })?;
                if let Name::Module { name, .. } = subsection {
                    module_name = Some(name.to_string());
                    break;
                }
            }
        } else if section_name == "producers" {
            let producers_reader = ProducersSectionReader::new(reader.data(), reader.data_offset())
                .map_err(|e| {
                    DebuggerError::WasmLoadError(format!(
                        "Failed to parse producers section: {}",
                        e
                    ))
                })?;

            for field in producers_reader {
                let field = field.map_err(|e| {
                    DebuggerError::WasmLoadError(format!("Failed to read producers field: {}", e))
                })?;
                let values = field
                    .values
                    .into_iter()
                    .map(|value| {
                        value.map(|value| WasmProducerValue {
                            name: value.name.to_string(),
                            version: value.version.to_string(),
                        })
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| {
                        DebuggerError::WasmLoadError(format!(
                            "Failed to read producers values: {}",
                            e
                        ))
                    })?;
                producers.push(WasmProducerField {
                    name: field.name.to_string(),
                    values,
                });
            }
        }

        if is_debug_custom_section(section_name) {
            debug_section_bytes += reader.data().len();
            debug_sections.push(section_name.to_string());
        }
    }

    debug_sections.sort();
    let has_debug_sections = !debug_sections.is_empty();
    let (build_profile_hint, optimization_hint, likely_stripped, package_hints, heuristic_notes) =
        derive_artifact_heuristics(
            module_name.as_deref(),
            name_section_present,
            &debug_sections,
            &producers,
        );

    Ok(WasmArtifactMetadata {
        module_name,
        name_section_present,
        has_debug_sections,
        debug_sections,
        debug_section_bytes,
        producers,
        build_profile_hint,
        optimization_hint,
        likely_stripped,
        package_hints,
        heuristic_notes,
    })
}

impl ContractMetadata {
    /// Returns `true` when no metadata fields have been populated.
    pub fn is_empty(&self) -> bool {
        self.contract_version.is_none()
            && self.sdk_version.is_none()
            && self.build_date.is_none()
            && self.author.is_none()
            && self.description.is_none()
            && self.implementation.is_none()
    }
}

/// Serde-compatible intermediate type for parsing JSON metadata payloads.
///
/// Both snake_case and camelCase field names are accepted for flexibility.
#[derive(Debug, Default, Deserialize)]
struct JsonContractMetadata {
    #[serde(alias = "contract_version", alias = "contractVersion")]
    contract_version: Option<String>,

    #[serde(alias = "sdk_version", alias = "sdkVersion")]
    sdk_version: Option<String>,

    #[serde(alias = "build_date", alias = "buildDate")]
    build_date: Option<String>,

    #[serde(alias = "author", alias = "organisation", alias = "organization")]
    author: Option<String>,

    #[serde(alias = "description")]
    description: Option<String>,

    #[serde(
        alias = "implementation",
        alias = "implementation_notes",
        alias = "implementationNotes"
    )]
    implementation: Option<String>,
}

impl From<JsonContractMetadata> for ContractMetadata {
    fn from(j: JsonContractMetadata) -> Self {
        ContractMetadata {
            contract_version: j.contract_version,
            sdk_version: j.sdk_version,
            build_date: j.build_date,
            author: j.author,
            description: j.description,
            implementation: j.implementation,
        }
    }
}

// ─── metadata extraction ──────────────────────────────────────────────────────

/// Extract contract metadata from WASM custom sections.
///
/// Searches for a `contractmeta` custom section containing UTF-8 text.  The
/// payload is first interpreted as JSON; if that fails, a permissive
/// `key: value` / `key=value` line-based format is attempted.
///
/// Contracts that embed no metadata return an empty [`ContractMetadata`]
/// without error.
pub fn extract_contract_metadata(wasm_bytes: &[u8]) -> Result<ContractMetadata> {
    let mut metadata = ContractMetadata::default();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        let Payload::CustomSection(reader) = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?
        else {
            continue;
        };

        if reader.name() != "contractmeta" {
            continue;
        }

        let data = reader.data();
        let Ok(text) = std::str::from_utf8(data) else {
            // Non-UTF-8 custom section data is skipped silently.
            continue;
        };

        // ── attempt JSON deserialization first ────────────────────────────
        if let Ok(json_meta) = serde_json::from_str::<JsonContractMetadata>(text) {
            let parsed: ContractMetadata = json_meta.into();

            if metadata.contract_version.is_none() {
                metadata.contract_version = parsed.contract_version;
            }
            if metadata.sdk_version.is_none() {
                metadata.sdk_version = parsed.sdk_version;
            }
            if metadata.build_date.is_none() {
                metadata.build_date = parsed.build_date;
            }
            if metadata.author.is_none() {
                metadata.author = parsed.author;
            }
            if metadata.description.is_none() {
                metadata.description = parsed.description;
            }
            if metadata.implementation.is_none() {
                metadata.implementation = parsed.implementation;
            }

            if !metadata.is_empty() {
                break;
            }

            continue;
        }

        // ── fallback: "key: value" / "key=value" line-based format ────────
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let (key, value) = if let Some((k, v)) = line.split_once('=') {
                (k.trim(), v.trim())
            } else if let Some((k, v)) = line.split_once(':') {
                (k.trim(), v.trim())
            } else {
                continue;
            };

            match key {
                "contract_version" | "contractVersion" if metadata.contract_version.is_none() => {
                    metadata.contract_version = Some(value.to_string());
                }
                "sdk_version" | "sdkVersion" if metadata.sdk_version.is_none() => {
                    metadata.sdk_version = Some(value.to_string());
                }
                "build_date" | "buildDate" if metadata.build_date.is_none() => {
                    metadata.build_date = Some(value.to_string());
                }
                "author" | "organisation" | "organization" if metadata.author.is_none() => {
                    metadata.author = Some(value.to_string());
                }
                "description" if metadata.description.is_none() => {
                    metadata.description = Some(value.to_string());
                }
                "implementation" | "implementation_notes" | "implementationNotes"
                    if metadata.implementation.is_none() =>
                {
                    metadata.implementation = Some(value.to_string());
                }
                _ => {}
            }
        }
    }

    Ok(metadata)
}

// ─── contract spec / function signatures ─────────────────────────────────────

/// A single function parameter: name and its Soroban type as a display string.
/// A function parameter for a contract spec-level signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionParam {
    pub name: String,
    pub type_name: String,
}

/// A full contract-spec-level signature for one exported contract function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractFunctionSignature {
    pub name: String,
    pub params: Vec<FunctionParam>,
    pub return_type: Option<String>,
}

/// A custom error definition extracted from a contract spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CustomError {
    pub code: u32,
    pub name: String,
    pub doc: String,
}

/// Convert an XDR `ScSpecTypeDef` into a human-readable type string.
fn spec_type_to_string(ty: &stellar_xdr::curr::ScSpecTypeDef) -> String {
    use stellar_xdr::curr::ScSpecTypeDef as T;
    match ty {
        T::Val => "Val".into(),
        T::Bool => "Bool".into(),
        T::Void => "Void".into(),
        T::Error => "Error".into(),
        T::U32 => "U32".into(),
        T::I32 => "I32".into(),
        T::U64 => "U64".into(),
        T::I64 => "I64".into(),
        T::Timepoint => "Timepoint".into(),
        T::Duration => "Duration".into(),
        T::U128 => "U128".into(),
        T::I128 => "I128".into(),
        T::U256 => "U256".into(),
        T::I256 => "I256".into(),
        T::Bytes => "Bytes".into(),
        T::String => "String".into(),
        T::Symbol => "Symbol".into(),
        T::Address => "Address".into(),
        T::Option(o) => format!("Option<{}>", spec_type_to_string(&o.value_type)),
        T::Result(r) => format!(
            "Result<{}, {}>",
            spec_type_to_string(&r.ok_type),
            spec_type_to_string(&r.error_type),
        ),
        T::Vec(v) => format!("Vec<{}>", spec_type_to_string(&v.element_type)),
        T::Map(m) => format!(
            "Map<{}, {}>",
            spec_type_to_string(&m.key_type),
            spec_type_to_string(&m.value_type),
        ),
        T::Tuple(t) => {
            let inner: Vec<String> = t.value_types.iter().map(spec_type_to_string).collect();
            format!("Tuple<{}>", inner.join(", "))
        }
        T::BytesN(b) => format!("BytesN<{}>", b.n),
        T::Udt(u) => std::str::from_utf8(u.name.as_slice())
            .unwrap_or("Udt")
            .to_string(),
    }
}

/// Helper: convert a `StringM<N>` slice to an owned `String` lossily.
fn stringm_to_string(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes)
        .unwrap_or("<invalid utf8>")
        .to_string()
}

/// Parse full function signatures from the WASM `contractspecv0` custom section.
///
/// Returns an empty `Vec` (not an error) when no spec section is present —
/// this keeps callers simple and backward-compatible with contracts that
/// pre-date the spec section.
pub fn parse_function_signatures(wasm_bytes: &[u8]) -> Result<Vec<ContractFunctionSignature>> {
    use stellar_xdr::curr::{Limited, Limits, ReadXdr, ScSpecEntry};

    let mut signatures = Vec::new();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        let Payload::CustomSection(reader) = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?
        else {
            continue;
        };

        if reader.name() != "contractspecv0" {
            continue;
        }

        let data = reader.data();
        let cursor = std::io::Cursor::new(data);
        let mut limited = Limited::new(cursor, Limits::none());

        // The section is a packed sequence of XDR-encoded ScSpecEntry values.
        loop {
            match ScSpecEntry::read_xdr(&mut limited) {
                Ok(ScSpecEntry::FunctionV0(func)) => {
                    let name = stringm_to_string(func.name.0.as_slice());

                    let params = func
                        .inputs
                        .iter()
                        .map(|input| FunctionParam {
                            name: stringm_to_string(input.name.as_slice()),
                            type_name: spec_type_to_string(&input.type_),
                        })
                        .collect();

                    let return_type = func.outputs.first().map(spec_type_to_string);

                    signatures.push(ContractFunctionSignature {
                        name,
                        params,
                        return_type,
                    });
                }
                Ok(_) => {
                    // UDT definitions, events, etc. — skip
                }
                Err(_) => break, // end of section or corrupt data
            }
        }

        break; // only one contractspecv0 section exists per contract
    }

    Ok(signatures)
}

#[allow(dead_code)]
fn val_type_to_wasm_type(vt: &ValType) -> WasmType {
    match vt {
        ValType::I32 => WasmType::I32,
        ValType::I64 => WasmType::I64,
        ValType::F32 => WasmType::F32,
        ValType::F64 => WasmType::F64,
        ValType::V128 => WasmType::V128,
        ValType::Ref(rt) => {
            if rt.is_func_ref() {
                WasmType::FuncRef
            } else if rt.is_extern_ref() {
                WasmType::ExternRef
            } else {
                WasmType::Unknown
            }
        }
    }
}
/// Parse custom error definitions from the WASM `contractspecv0` custom section.
pub fn parse_custom_errors(wasm_bytes: &[u8]) -> Result<Vec<CustomError>> {
    use stellar_xdr::curr::{Limited, Limits, ReadXdr, ScSpecEntry};

    let mut errors = Vec::new();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        let Payload::CustomSection(reader) = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?
        else {
            continue;
        };

        if reader.name() != "contractspecv0" {
            continue;
        }

        let data = reader.data();
        let cursor = std::io::Cursor::new(data);
        let mut limited = Limited::new(cursor, Limits::none());

        loop {
            match ScSpecEntry::read_xdr(&mut limited) {
                Ok(ScSpecEntry::UdtErrorEnumV0(err_enum)) => {
                    for case in err_enum.cases.iter() {
                        errors.push(CustomError {
                            code: case.value,
                            name: stringm_to_string(case.name.as_slice()),
                            doc: stringm_to_string(case.doc.as_slice()),
                        });
                    }
                }
                Ok(_) => {
                    // Other spec entries — skip
                }
                Err(_) => break, // end of section or corrupt data
            }
        }

        break;
    }

    Ok(errors)
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── SHA-256 tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_compute_wasm_sha256_known_value() {
        let input = b"hello world";
        // Pre-computed SHA-256 for "hello world"
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert_eq!(compute_wasm_sha256(input), expected);
    }

    #[test]
    fn test_compute_wasm_sha256_empty_input() {
        let input: &[u8] = &[];
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(compute_wasm_sha256(input), expected);
    }

    #[test]
    fn test_compute_wasm_sha256_deterministic() {
        let input = b"deterministic input";
        let hash1 = compute_wasm_sha256(input);
        let hash2 = compute_wasm_sha256(input);
        assert_eq!(hash1, hash2);
    }

    // ── Checksum verification tests ───────────────────────────────────────────

    #[test]
    fn test_expected_hash_match_proceeds() {
        let computed = "abcdef123456";
        let expected = Some("abcdef123456".to_string());
        // Verify no error is returned
        assert!(verify_wasm_hash(computed, expected.as_ref()).is_ok());
    }

    #[test]
    fn test_expected_hash_mismatch_returns_error() {
        let computed = "abcdef123456";
        let expected = Some("wronghash999".to_string());
        let result = verify_wasm_hash(computed, expected.as_ref());

        assert!(result.is_err());
        let err = result.unwrap_err();
        // Downcast back to DebuggerError to check the variant
        match err.downcast_ref::<crate::DebuggerError>() {
            Some(crate::DebuggerError::ChecksumMismatch(e, a)) => {
                assert_eq!(e, "wronghash999");
                assert_eq!(a, "abcdef123456");
            }
            _ => panic!("Expected ChecksumMismatch error"),
        }
    }

    #[test]
    fn test_expected_hash_none_skips_check() {
        let computed = "abcdef123456";
        // When None is passed, it should proceed without error
        assert!(verify_wasm_hash(computed, None).is_ok());
    }

    #[test]
    fn test_checksum_mismatch_message_includes_sha256() {
        // #1271: hash algorithm context must be explicit in mismatch errors so
        // callers can supply the right hash format.
        let computed = "abcdef123456";
        let expected = Some("wronghash999".to_string());
        let err = verify_wasm_hash(computed, expected.as_ref()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("sha256"),
            "expected sha256 algorithm context in error, got: {msg}"
        );
        assert!(
            msg.contains("wronghash999"),
            "expected hash missing from message: {msg}"
        );
        assert!(
            msg.contains("abcdef123456"),
            "computed hash missing from message: {msg}"
        );
    }

    // ── WASM test-module builder ──────────────────────────────────────────────

    /// Encode `value` as an unsigned LEB128 byte sequence.
    ///
    /// WASM mandates LEB128 for all integer fields in the binary format,
    /// including section sizes and string lengths.  A plain `as u8` cast is
    /// only valid for values 0–127; anything larger requires multiple bytes.
    fn uleb128(mut value: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            // Take the 7 low-order bits.
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            // Set the continuation bit when more bytes follow.
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }

    /// Build a minimal valid WASM module that contains a single custom section.
    ///
    /// Uses proper ULEB128 encoding so it works for payloads of any size,
    /// unlike a naïve single-byte length which panics above 127 bytes.
    fn make_custom_section_wasm(name: &str, payload: &[u8]) -> Vec<u8> {
        make_wasm_with_custom_sections(&[(name, payload)])
    }

    fn make_wasm_with_custom_sections(sections: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();

        // WASM magic number and version.
        bytes.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]);
        bytes.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        for (name, payload) in sections {
            // Section id 0 = custom section.
            bytes.push(0x00);

            // Section content: LEB128(name.len) ++ name ++ payload.
            let mut section = Vec::new();
            section.extend_from_slice(&uleb128(name.len()));
            section.extend_from_slice(name.as_bytes());
            section.extend_from_slice(payload);

            // Section size as LEB128, then the content.
            bytes.extend_from_slice(&uleb128(section.len()));
            bytes.extend_from_slice(&section);
        }

        bytes
    }

    fn make_name_section_payload(module_name: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.push(0x00); // module name subsection

        let mut subsection = Vec::new();
        subsection.extend_from_slice(&uleb128(module_name.len()));
        subsection.extend_from_slice(module_name.as_bytes());

        payload.extend_from_slice(&uleb128(subsection.len()));
        payload.extend_from_slice(&subsection);
        payload
    }

    fn make_producers_section_payload(fields: &[(&str, &[(&str, &str)])]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&uleb128(fields.len()));

        for (field_name, values) in fields {
            payload.extend_from_slice(&uleb128(field_name.len()));
            payload.extend_from_slice(field_name.as_bytes());
            payload.extend_from_slice(&uleb128(values.len()));
            for (name, version) in *values {
                payload.extend_from_slice(&uleb128(name.len()));
                payload.extend_from_slice(name.as_bytes());
                payload.extend_from_slice(&uleb128(version.len()));
                payload.extend_from_slice(version.as_bytes());
            }
        }

        payload
    }

    fn encode_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&uleb128(value.len()));
        bytes.extend_from_slice(value.as_bytes());
    }

    fn append_section(module: &mut Vec<u8>, id: u8, section: &[u8]) {
        module.push(id);
        module.extend_from_slice(&uleb128(section.len()));
        module.extend_from_slice(section);
    }

    fn make_wasm_with_cross_contract_call() -> Vec<u8> {
        let mut module = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

        // Type section: one () -> () function type.
        let mut ty = Vec::new();
        ty.extend_from_slice(&uleb128(1));
        ty.push(0x60);
        ty.push(0x00);
        ty.push(0x00);
        append_section(&mut module, 1, &ty);

        // Import section: import function env::invoke_contract.
        let mut import = Vec::new();
        import.extend_from_slice(&uleb128(1));
        encode_string(&mut import, "env");
        encode_string(&mut import, "invoke_contract");
        import.push(0x00); // import kind: function
        import.extend_from_slice(&uleb128(0)); // type index 0
        append_section(&mut module, 2, &import);

        // Function section: one local function using type index 0.
        let mut functions = Vec::new();
        functions.extend_from_slice(&uleb128(1));
        functions.extend_from_slice(&uleb128(0));
        append_section(&mut module, 3, &functions);

        // Export section: export local function at index 1 (import is index 0).
        let mut exports = Vec::new();
        exports.extend_from_slice(&uleb128(1));
        encode_string(&mut exports, "entrypoint");
        exports.push(0x00); // export kind: function
        exports.extend_from_slice(&uleb128(1));
        append_section(&mut module, 7, &exports);

        // Code section: body = call imported function index 0; end.
        let mut code = Vec::new();
        code.extend_from_slice(&uleb128(1)); // one body
        let body = vec![0x00, 0x10, 0x00, 0x0b]; // no locals, call 0, end
        code.extend_from_slice(&uleb128(body.len()));
        code.extend_from_slice(&body);
        append_section(&mut module, 10, &code);

        module
    }

    // ── metadata-present tests ────────────────────────────────────────────────

    #[test]
    fn extract_metadata_from_json_custom_section() {
        let json = r#"
        {
            "contract_version": "1.2.3",
            "sdk_version": "22.0.0",
            "build_date": "2026-02-20",
            "author": "Example Org",
            "description": "Sample contract for testing",
            "implementation_notes": "Uses JSON metadata format"
        }
        "#;

        let wasm = make_custom_section_wasm("contractmeta", json.as_bytes());
        let meta = extract_contract_metadata(&wasm).expect("metadata should parse");

        assert_eq!(meta.contract_version.as_deref(), Some("1.2.3"));
        assert_eq!(meta.sdk_version.as_deref(), Some("22.0.0"));
        assert_eq!(meta.build_date.as_deref(), Some("2026-02-20"));
        assert_eq!(meta.author.as_deref(), Some("Example Org"));
        assert_eq!(
            meta.description.as_deref(),
            Some("Sample contract for testing")
        );
        assert_eq!(
            meta.implementation.as_deref(),
            Some("Uses JSON metadata format")
        );
    }

    #[test]
    fn extract_metadata_from_line_based_custom_section() {
        let text = "\
contract_version: 0.0.1
sdkVersion=22.0.0
build_date: 2026-02-19
author=Example Dev
description: Line based metadata
implementation_notes=Line-based format
";

        let wasm = make_custom_section_wasm("contractmeta", text.as_bytes());
        let meta = extract_contract_metadata(&wasm).expect("metadata should parse");

        assert_eq!(meta.contract_version.as_deref(), Some("0.0.1"));
        assert_eq!(meta.sdk_version.as_deref(), Some("22.0.0"));
        assert_eq!(meta.build_date.as_deref(), Some("2026-02-19"));
        assert_eq!(meta.author.as_deref(), Some("Example Dev"));
        assert_eq!(meta.description.as_deref(), Some("Line based metadata"));
        assert_eq!(meta.implementation.as_deref(), Some("Line-based format"));
    }

    // ── metadata-absent tests ─────────────────────────────────────────────────

    #[test]
    fn extract_metadata_from_wasm_without_metadata_section() {
        // Bare WASM header — no sections at all.
        let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let meta = extract_contract_metadata(&wasm).expect("parsing should succeed");
        assert!(meta.is_empty());
    }

    #[test]
    fn extract_metadata_ignores_unrelated_custom_sections() {
        // A custom section with a different name should not affect the result.
        let wasm = make_custom_section_wasm("some_other_section", b"irrelevant data");
        let meta = extract_contract_metadata(&wasm).expect("parsing should succeed");
        assert!(meta.is_empty());
    }

    #[test]
    fn extract_metadata_ignores_non_utf8_payload() {
        // Non-UTF-8 bytes in a contractmeta section must not cause an error.
        let bad_bytes: &[u8] = &[0xFF, 0xFE, 0x00, 0x01];
        let wasm = make_custom_section_wasm("contractmeta", bad_bytes);
        let meta = extract_contract_metadata(&wasm).expect("should not error");
        assert!(meta.is_empty());
    }

    #[test]
    fn parse_cross_contract_calls_detects_invoke_contract_import() {
        let wasm = make_wasm_with_cross_contract_call();
        let calls = parse_cross_contract_calls(&wasm).expect("should parse calls");

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].caller, "entrypoint");
        assert_eq!(calls[0].target, "external_contract");
        assert_eq!(calls[0].host_function, "invoke_contract");
    }

    #[test]
    fn test_get_module_info_with_sections() {
        let wasm = make_custom_section_wasm("test_section", &[0x01, 0x02, 0x03]);
        let info = get_module_info(&wasm).expect("should parse");

        assert_eq!(info.total_size, wasm.len());
        // Should have at least the custom section
        assert!(!info.sections.is_empty());
        let custom_section = info
            .sections
            .iter()
            .find(|s| s.name.contains("test_section"));
        assert!(custom_section.is_some());
        // Payload size: name length byte (1) + section name bytes (12) + data bytes (3).
        assert_eq!(custom_section.unwrap().size, 1 + 12 + 3);
    }

    #[test]
    fn contract_metadata_is_empty_when_default() {
        assert!(ContractMetadata::default().is_empty());
    }

    #[test]
    fn contract_metadata_not_empty_when_any_field_set() {
        let meta = ContractMetadata {
            contract_version: Some("1.0.0".into()),
            ..Default::default()
        };
        assert!(!meta.is_empty());
    }

    #[test]
    fn extract_wasm_artifact_metadata_detects_debug_sections_and_name_section() {
        let name_payload = make_name_section_payload("counter_contract");
        let wasm = make_wasm_with_custom_sections(&[
            ("name", &name_payload),
            (".debug_info", &[1, 2, 3, 4]),
            (".debug_line", &[5, 6]),
        ]);

        let artifact =
            extract_wasm_artifact_metadata(&wasm).expect("artifact metadata should parse");

        assert_eq!(artifact.module_name.as_deref(), Some("counter_contract"));
        assert!(artifact.name_section_present);
        assert!(artifact.has_debug_sections);
        assert_eq!(
            artifact.debug_sections,
            vec![".debug_info".to_string(), ".debug_line".to_string()]
        );
        assert_eq!(artifact.debug_section_bytes, 6);
        assert_eq!(artifact.build_profile_hint, "debug-like");
        assert!(artifact
            .heuristic_notes
            .iter()
            .any(|note| note.contains("DWARF-style debug sections detected")));
    }

    #[test]
    fn extract_wasm_artifact_metadata_parses_producers_and_optimization_hints() {
        let producers_payload = make_producers_section_payload(&[
            ("language", &[("rust", "1.78.0")]),
            (
                "processed-by",
                &[
                    ("cargo", "1.78.0"),
                    ("rustc", "1.78.0"),
                    ("wasm-opt", "116"),
                ],
            ),
            ("sdk", &[("soroban-sdk", "22.0.2")]),
        ]);
        let wasm = make_wasm_with_custom_sections(&[("producers", &producers_payload)]);

        let artifact =
            extract_wasm_artifact_metadata(&wasm).expect("artifact metadata should parse");

        assert!(!artifact.name_section_present);
        assert!(!artifact.has_debug_sections);
        assert_eq!(artifact.build_profile_hint, "stripped-release-like");
        assert_eq!(
            artifact.optimization_hint,
            "aggressively optimized (wasm-opt present)"
        );
        assert!(artifact.likely_stripped);
        assert_eq!(artifact.producers.len(), 3);
        assert!(artifact
            .package_hints
            .iter()
            .any(|hint| hint == "language: rust 1.78.0"));
        assert!(artifact
            .package_hints
            .iter()
            .any(|hint| hint == "sdk: soroban-sdk 22.0.2"));
        assert!(artifact
            .heuristic_notes
            .iter()
            .any(|note| note.contains("wasm-opt")));
    }

    // ── error extraction tests ────────────────────────────────────────────────

    #[test]
    fn extract_custom_errors() {
        use stellar_xdr::curr::{
            ScSpecEntry, ScSpecUdtErrorEnumCaseV0, ScSpecUdtErrorEnumV0, StringM, WriteXdr,
        };

        let case1 = ScSpecUdtErrorEnumCaseV0 {
            doc: StringM::try_from("My Error 1".as_bytes().to_vec()).unwrap(),
            name: StringM::try_from("ErrorOne".as_bytes().to_vec()).unwrap(),
            value: 100,
        };
        let case2 = ScSpecUdtErrorEnumCaseV0 {
            doc: StringM::try_from("My Error 2".as_bytes().to_vec()).unwrap(),
            name: StringM::try_from("ErrorTwo".as_bytes().to_vec()).unwrap(),
            value: 101,
        };
        let err_enum = ScSpecUdtErrorEnumV0 {
            doc: StringM::try_from("".as_bytes().to_vec()).unwrap(),
            lib: StringM::try_from("".as_bytes().to_vec()).unwrap(),
            name: StringM::try_from("MyErrorType".as_bytes().to_vec()).unwrap(),
            cases: vec![case1, case2].try_into().unwrap(),
        };

        let entry = ScSpecEntry::UdtErrorEnumV0(err_enum);
        let payload = entry.to_xdr(stellar_xdr::curr::Limits::none()).unwrap();

        let wasm = make_custom_section_wasm("contractspecv0", &payload);

        let errors = parse_custom_errors(&wasm).expect("parsing should succeed");
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].code, 100);
        assert_eq!(errors[0].name, "ErrorOne");
        assert_eq!(errors[0].doc, "My Error 1");
        assert_eq!(errors[1].code, 101);
        assert_eq!(errors[1].name, "ErrorTwo");
        assert_eq!(errors[1].doc, "My Error 2");
    }
}
