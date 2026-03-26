use crate::runtime::executor::ContractExecutor;
use crate::server::protocol::{DynamicTraceEvent, DynamicTraceEventKind};
use crate::utils::wasm::{parse_instructions, WasmInstruction};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use wasmparser::{Operator, Parser, Payload};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Severity {
    #[default]
    Low,
    Medium,
    High,
}

#[derive(Debug, Default, Clone)]
pub struct AnalyzerFilter {
    pub enable_rules: Vec<String>,
    pub disable_rules: Vec<String>,
    pub min_severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub rule_id: String,
    pub severity: Severity,
    pub location: String,
    pub description: String,
    pub remediation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityReport {
    pub findings: Vec<SecurityFinding>,
}

pub trait SecurityRule {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn analyze_static(&self, _wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        Ok(vec![])
    }
    fn analyze_dynamic(
        &self,
        _executor: Option<&ContractExecutor>,
        _trace: &[DynamicTraceEvent],
    ) -> Result<Vec<SecurityFinding>> {
        Ok(vec![])
    }
}

pub struct SecurityAnalyzer {
    rules: Vec<Box<dyn SecurityRule>>,
}

impl SecurityAnalyzer {
    pub fn new() -> Self {
        Self {
            rules: vec![
                Box::new(HardcodedAddressRule),
                Box::new(ArithmeticCheckRule),
                Box::new(AuthorizationCheckRule),
                Box::new(ReentrancyPatternRule),
                Box::new(CrossContractImportRule),
                Box::new(UnboundedIterationRule),
            ],
        }
    }

    pub fn analyze(
        &self,
        wasm_bytes: &[u8],
        executor: Option<&ContractExecutor>,
        trace: Option<&[DynamicTraceEvent]>,
        filter: &AnalyzerFilter,
    ) -> Result<SecurityReport> {
        let mut report = SecurityReport::default();

        for rule in &self.rules {
            let name = rule.name();

            if !filter.enable_rules.is_empty() && !filter.enable_rules.iter().any(|r| r == name) {
                continue;
            }
            if filter.disable_rules.iter().any(|r| r == name) {
                continue;
            }

            let static_findings = rule.analyze_static(wasm_bytes)?;
            report.findings.extend(
                static_findings
                    .into_iter()
                    .filter(|f| f.severity >= filter.min_severity),
            );

            if let Some(tr) = trace {
                let dynamic_findings = rule.analyze_dynamic(executor, tr)?;
                report.findings.extend(
                    dynamic_findings
                        .into_iter()
                        .filter(|f| f.severity >= filter.min_severity),
                );
            }
        }

        Ok(report)
    }
}

impl Default for SecurityAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// StrKey validation helpers
// ---------------------------------------------------------------------------

/// CRC-16/XModem (poly = 0x1021, init = 0x0000, no reflection).
/// Used by Stellar StrKey to protect against transcription errors.
fn strkey_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0x0000;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if (crc & 0x8000) != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Returns `true` only when `s` is a cryptographically valid Stellar StrKey.
///
/// Validation steps (SEP-0023):
///   1. Must be exactly 56 characters, all from the base32 alphabet (A–Z, 2–7).
///   2. Base32-decode to exactly 35 bytes.
///   3. `decoded[0]` must be a recognised version byte:
///      • `0x30` (6 << 3) → ED25519 public key  → 'G' prefix
///      • `0x10` (2 << 3) → contract address    → 'C' prefix
///   4. CRC-16/XModem over `decoded[0..33]` must equal the little-endian u16
///      stored in `decoded[33..35]`.
///
/// Any 56-char string that fails even one of these steps is **not** a valid
/// address — it is, for example, an error-message fragment, a base64 blob, or
/// a random identifier that merely happens to start with 'G' or 'C'.
fn is_valid_strkey(s: &str) -> bool {
    if s.len() != 56 {
        return false;
    }

    // --- Base32 decode (RFC 4648, no padding) ---
    // 56 chars × 5 bits = 280 bits = 35 bytes exactly.
    let mut decoded = [0u8; 35];
    let mut bits: u64 = 0;
    let mut bit_count: u32 = 0;
    let mut byte_idx: usize = 0;

    for ch in s.bytes() {
        let val: u64 = match ch {
            b'A'..=b'Z' => (ch - b'A') as u64,
            b'2'..=b'7' => (ch - b'2' + 26) as u64,
            _ => {
                return false;
            } // character outside base32 alphabet
        };
        bits = (bits << 5) | val;
        bit_count += 5;
        if bit_count >= 8 {
            bit_count -= 8;
            if byte_idx >= 35 {
                return false;
            }
            decoded[byte_idx] = ((bits >> bit_count) & 0xff) as u8;
            byte_idx += 1;
        }
    }

    if byte_idx != 35 {
        return false;
    }

    // --- Version byte ---
    let version = decoded[0];
    if version != (6u8 << 3) && version != (2u8 << 3) {
        return false;
    }

    // --- Checksum ---
    let expected = u16::from_le_bytes([decoded[33], decoded[34]]);
    let computed = strkey_crc16(&decoded[..33]);
    computed == expected
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

struct HardcodedAddressRule;
impl SecurityRule for HardcodedAddressRule {
    fn name(&self) -> &str {
        "hardcoded-address"
    }
    fn description(&self) -> &str {
        "Detects hardcoded Stellar addresses in WASM data sections."
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();

        for payload in Parser::new(0).parse_all(wasm_bytes).flatten() {
            if let Payload::DataSection(reader) = payload {
                for data in reader.into_iter().flatten() {
                    let content = String::from_utf8_lossy(data.data);
                    for word in content.split(|c: char| !c.is_alphanumeric()) {
                        // Guard 1 – fast pre-filter (cheap): right length and prefix.
                        // Guard 2 – full StrKey validation (base32 + version byte + CRC-16).
                        //
                        // Without guard 2, arbitrary 56-char constants such as error
                        // message fragments or base64 blobs that happen to start with
                        // 'G' or 'C' would be mis-classified as addresses.
                        if (word.starts_with('G') || word.starts_with('C'))
                            && word.len() == 56
                            && is_valid_strkey(word)
                        {
                            findings.push(SecurityFinding {
                                rule_id: self.name().to_string(),
                                severity: Severity::Medium,
                                location: "Data Section".to_string(),
                                description: format!("Found potential hardcoded address: {}", word),
                                remediation:
                                    "Use Address::from_str from configuration or function \
                                     arguments instead of hardcoding."
                                        .to_string(),
                                confidence: None,
                                rationale: None,
                            });
                        }
                    }
                }
            }
        }
        Ok(findings)
    }
}

struct ArithmeticCheckRule;
impl SecurityRule for ArithmeticCheckRule {
    fn name(&self) -> &str {
        "arithmetic-overflow"
    }
    fn description(&self) -> &str {
        "Detects potential for unchecked arithmetic overflow."
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();
        let instructions = parse_instructions(wasm_bytes);

        for (i, instr) in instructions.iter().enumerate() {
            if Self::is_arithmetic(instr) && !Self::is_guarded(&instructions, i) {
                findings.push(SecurityFinding {
                    rule_id: self.name().to_string(),
                    severity: Severity::Medium,
                    location: format!("Instruction {}", i),
                    description: format!("Unchecked arithmetic operation detected: {:?}", instr),
                    remediation: "Ensure arithmetic operations are guarded with proper bounds checks or overflow handling.".to_string(),
                    confidence: None,
                    rationale: None,
                });
            }
        }

        Ok(findings)
    }
}

impl ArithmeticCheckRule {
    fn is_arithmetic(instr: &WasmInstruction) -> bool {
        matches!(
            instr,
            WasmInstruction::I32Add
                | WasmInstruction::I32Sub
                | WasmInstruction::I32Mul
                | WasmInstruction::I64Add
                | WasmInstruction::I64Sub
                | WasmInstruction::I64Mul
        )
    }

    fn is_guarded(instructions: &[WasmInstruction], arithmetic_idx: usize) -> bool {
        if arithmetic_idx + 1 >= instructions.len() {
            return false;
        }

        let window_end = (arithmetic_idx + 4).min(instructions.len());
        instructions[arithmetic_idx + 1..window_end]
            .iter()
            .any(|instr| matches!(instr, WasmInstruction::If | WasmInstruction::BrIf))
    }
}

struct AuthorizationCheckRule;
impl SecurityRule for AuthorizationCheckRule {
    fn name(&self) -> &str {
        "missing-auth"
    }
    fn description(&self) -> &str {
        "Detects sensitive flows missing authorization checks."
    }

    fn analyze_dynamic(
        &self,
        _executor: Option<&ContractExecutor>,
        trace: &[DynamicTraceEvent],
    ) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();
        let mut auth_sequence = None;
        let mut problematic_storage_writes = Vec::new();

        // First pass: find the earliest authorization event and any storage writes before it
        for entry in trace {
            if entry.kind == DynamicTraceEventKind::Authorization {
                // Record the earliest authorization event
                match auth_sequence {
                    None => auth_sequence = Some(entry.sequence),
                    Some(current_auth_seq) => {
                        if entry.sequence < current_auth_seq {
                            auth_sequence = Some(entry.sequence);
                        }
                    }
                }
            } else if entry.kind == DynamicTraceEventKind::StorageWrite {
                // Check if this storage write happens before any authorization
                if let Some(auth_seq) = auth_sequence {
                    if entry.sequence < auth_seq {
                        problematic_storage_writes.push(entry.sequence);
                    }
                } else {
                    // No auth seen yet, this storage write is problematic
                    problematic_storage_writes.push(entry.sequence);
                }
            }
        }

        // If we have storage writes without preceding auth, report a finding
        if !problematic_storage_writes.is_empty() {
            findings.push(SecurityFinding {
                rule_id: self.name().to_string(),
                severity: Severity::High,
                location: "Dynamic trace".to_string(),
                description: format!(
                    "Storage mutation detected without preceding authorization. Found {} storage write(s) occurring before any authorization event.",
                    problematic_storage_writes.len()
                ),
                remediation: "Ensure all sensitive functions call `address.require_auth()` before mutating state.".to_string(),
                confidence: None,
                rationale: None,
            });
        }

        Ok(findings)
    }
}

struct ReentrancyPatternRule;
impl SecurityRule for ReentrancyPatternRule {
    fn name(&self) -> &str {
        "reentrancy-pattern"
    }
    fn description(&self) -> &str {
        "Detects cross-contract calls followed by storage writes in the same call frame."
    }

    fn analyze_dynamic(
        &self,
        _executor: Option<&ContractExecutor>,
        trace: &[DynamicTraceEvent],
    ) -> Result<Vec<SecurityFinding>> {
        Ok(analyze_reentrancy_pattern_dynamic(trace))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FrameKey {
    function: Option<String>,
    call_depth: Option<usize>,
}

#[derive(Debug, Clone)]
struct PendingCrossCall {
    frame: Option<FrameKey>,
    sequence: usize,
    pre_call_write_seen: bool,
    inferred: bool,
}

struct CrossContractImportRule;
impl SecurityRule for CrossContractImportRule {
    fn name(&self) -> &str {
        "cross-contract-import"
    }

    fn description(&self) -> &str {
        "Detects cross-contract host function imports with robust name matching."
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let mut matches = Vec::new();

        for payload in Parser::new(0).parse_all(wasm_bytes) {
            let Ok(payload) = payload else {
                // Many unit tests feed non-module bytes into the analyzer. Degrade gracefully.
                return Ok(Vec::new());
            };

            if let Payload::ImportSection(reader) = payload {
                for import in reader.into_iter() {
                    let Ok(import) = import else {
                        continue;
                    };

                    if !matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                        continue;
                    }

                    if is_cross_contract_host_import(import.module, import.name) {
                        matches.push(format!("{}::{}", import.module, import.name));
                    }
                }
            }
        }

        if matches.is_empty() {
            return Ok(Vec::new());
        }

        Ok(vec![SecurityFinding {
            rule_id: self.name().to_string(),
            severity: Severity::Low,
            location: "Import Section".to_string(),
            description: format!(
                "Cross-contract host imports detected: {}",
                matches.join(", ")
            ),
            remediation: "Review external call sites for reentrancy and authorization checks."
                .to_string(),
            confidence: None,
            rationale: None,
        }])
    }
}

fn canonicalize_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

fn is_env_like_module(module: &str) -> bool {
    let m = canonicalize_ascii(module);
    m == "env" || m.starts_with("sorobanenv")
}

fn is_cross_contract_host_function_name(name: &str) -> bool {
    const BASES: &[&str] = &[
        "invokecontract",
        "tryinvokecontract",
        "callcontract",
        "trycallcontract",
        "trycall",
    ];

    let n = canonicalize_ascii(name);
    for base in BASES {
        if n == *base {
            return true;
        }
        if let Some(suffix) = n.strip_prefix(base) {
            if suffix.is_empty() {
                return true;
            }
            if let Some(rest) = suffix.strip_prefix('v') {
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
        }
    }

    false
}

fn is_cross_contract_host_import(module: &str, name: &str) -> bool {
    is_env_like_module(module) && is_cross_contract_host_function_name(name)
}

struct UnboundedIterationRule;
impl SecurityRule for UnboundedIterationRule {
    fn name(&self) -> &str {
        "unbounded-iteration"
    }
    fn description(&self) -> &str {
        "Detects storage-driven loops and unbounded read patterns."
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let analysis = analyze_unbounded_iteration_static(wasm_bytes);
        if !analysis.suspicious {
            return Ok(Vec::new());
        }

        let mut finding = SecurityFinding {
            rule_id: self.name().to_string(),
            severity: Severity::High,
            location: "WASM code section".to_string(),
            description: format!(
                "Detected loop(s) with storage-read host calls ({} storage calls while inside loop).",
                analysis.storage_calls_inside_loops
            ),
            remediation: "Bound iteration over storage-backed collections (pagination, explicit limits, or capped batch size).".to_string(),
            confidence: analysis.confidence,
            rationale: analysis.rationale.clone(),
        };

        if analysis.max_nesting_depth > 1 {
            finding.description = format!(
                "{} Loop nesting depth: {} (increased complexity).",
                finding.description, analysis.max_nesting_depth
            );
        }

        Ok(vec![finding])
    }

    fn analyze_dynamic(
        &self,
        _executor: Option<&ContractExecutor>,
        trace: &[DynamicTraceEvent],
    ) -> Result<Vec<SecurityFinding>> {
        Ok(analyze_unbounded_iteration_dynamic(trace)
            .into_iter()
            .map(|mut finding| {
                finding.rule_id = self.name().to_string();
                finding
            })
            .collect())
    }
}

#[derive(Debug, Default)]
struct UnboundedStaticSignal {
    suspicious: bool,
    storage_calls_inside_loops: usize,
    confidence: Option<f32>,
    rationale: Option<String>,
    loop_types: Vec<String>,
    max_nesting_depth: usize,
}

#[derive(Debug, Clone)]
enum ControlFlowFrame {
    Loop { loop_type: String },
    Block,
    If,
}

impl ControlFlowFrame {
    fn is_loop(&self) -> bool {
        matches!(self, ControlFlowFrame::Loop { .. })
    }

    fn loop_type(&self) -> Option<&str> {
        match self {
            ControlFlowFrame::Loop { loop_type, .. } => Some(loop_type),
            _ => None,
        }
    }
}

fn analyze_unbounded_iteration_static(wasm_bytes: &[u8]) -> UnboundedStaticSignal {
    let mut storage_import_indices = HashSet::new();
    let mut imported_func_count = 0u32;
    let mut control_flow_stack: Vec<ControlFlowFrame> = Vec::new();
    let mut signal = UnboundedStaticSignal::default();

    let mut storage_calls_in_loops = 0usize;
    let mut _storage_calls_outside_loops = 0usize;
    let mut loop_types_with_calls: HashSet<String> = HashSet::new();
    let mut loop_types_seen: HashSet<String> = HashSet::new();
    let mut _conditional_branches = 0usize;

    for payload in Parser::new(0).parse_all(wasm_bytes) {
        let Ok(payload) = payload else {
            return signal;
        };

        match payload {
            Payload::ImportSection(reader) => {
                for import in reader.into_iter().flatten() {
                    if let wasmparser::TypeRef::Func(_) = import.ty {
                        if is_storage_read_import(import.module, import.name) {
                            storage_import_indices.insert(imported_func_count);
                        }
                        imported_func_count += 1;
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let Ok(mut operators) = body.get_operators_reader() else {
                    continue;
                };

                while !operators.eof() {
                    let Ok(op) = operators.read() else {
                        break;
                    };

                    match op {
                        Operator::Loop { .. } => {
                            let current_depth =
                                control_flow_stack.iter().filter(|f| f.is_loop()).count();
                            let loop_type = (if current_depth > 0 {
                                "nested_loop"
                            } else {
                                "top_level_loop"
                            })
                            .to_string();
                            loop_types_seen.insert(loop_type.clone());

                            control_flow_stack.push(ControlFlowFrame::Loop {
                                loop_type: loop_type.clone(),
                            });
                            signal.max_nesting_depth =
                                signal.max_nesting_depth.max(current_depth + 1);
                        }
                        Operator::Block { .. } => {
                            control_flow_stack.push(ControlFlowFrame::Block);
                        }
                        Operator::If { .. } => {
                            _conditional_branches += 1;
                            control_flow_stack.push(ControlFlowFrame::If);
                        }
                        Operator::Else => {}
                        Operator::End => {
                            if let Some(_frame) = control_flow_stack.pop() {
                                // max_nesting_depth tracks the peak depth and shouldn't be decremented
                            }
                        }
                        Operator::Call { function_index } => {
                            let is_storage_call = storage_import_indices.contains(&function_index);
                            let current_loop_depth =
                                control_flow_stack.iter().filter(|f| f.is_loop()).count();

                            if is_storage_call {
                                if current_loop_depth > 0 {
                                    storage_calls_in_loops += 1;
                                    if let Some(loop_frame) =
                                        control_flow_stack.iter().rev().find(|f| f.is_loop())
                                    {
                                        if let Some(loop_type) = loop_frame.loop_type() {
                                            loop_types_with_calls.insert(loop_type.to_string());
                                        }
                                    }
                                } else {
                                    _storage_calls_outside_loops += 1;
                                }
                            }
                        }
                        Operator::BrIf { .. } => {
                            _conditional_branches += 1;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    signal.storage_calls_inside_loops = storage_calls_in_loops;
    signal.loop_types = loop_types_seen.into_iter().collect();

    // Calculate confidence based on multiple factors
    let confidence = if storage_calls_in_loops > 0 {
        if signal.max_nesting_depth >= 2 && storage_calls_in_loops >= 3 {
            0.9
        } else if signal.max_nesting_depth > 1 || storage_calls_in_loops > 1 {
            0.7
        } else {
            0.5
        }
    } else {
        0.2
    };

    signal.rationale = Some(format!(
        "Storage calls in loops: {}, max nesting depth: {}, loop types with calls: {:?}",
        storage_calls_in_loops, signal.max_nesting_depth, loop_types_with_calls
    ));

    signal.confidence = Some(confidence);

    signal.suspicious = storage_calls_in_loops > 0;
    signal
}

fn is_storage_read_import(module: &str, name: &str) -> bool {
    const BASES: &[&str] = &[
        "storageget",
        "storagehas",
        "storagenext",
        "storageiter",
        "getcontractdata",
        "hascontractdata",
        "mapget",
        "vecget",
        "contractstorageget",
        "sorobanstoragehas",
    ];

    if !is_env_like_module(module) {
        return false;
    }

    let n = canonicalize_ascii(name);
    for base in BASES {
        if n == *base {
            return true;
        }
        // Handle prefix-qualified names like "contract_storage_get" or "soroban_storage_has"
        if n.ends_with(base) {
            return true;
        }
        if let Some(suffix) = n.strip_prefix(base) {
            if suffix.is_empty() {
                return true;
            }
            if let Some(rest) = suffix.strip_prefix('v') {
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                    return true;
                }
            }
        }

        // Handle prefix-qualified names like "contract_storage_get".
        if n.ends_with(base) {
            return true;
        }
    }

    false
}

fn analyze_unbounded_iteration_dynamic(trace: &[DynamicTraceEvent]) -> Option<SecurityFinding> {
    let mut read_key_counts: HashMap<&str, usize> = HashMap::new();
    let mut total_reads = 0usize;

    for entry in trace {
        if entry.kind == DynamicTraceEventKind::StorageRead {
            total_reads += 1;
            if let Some(key) = entry.storage_key.as_deref() {
                *read_key_counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    if total_reads == 0 {
        return None;
    }

    let unique_keys = read_key_counts.len();
    let max_reads_for_one_key = read_key_counts.values().copied().max().unwrap_or(0);
    let likely_unbounded = total_reads >= 64
        && (unique_keys <= total_reads / 4 || max_reads_for_one_key >= 32 || total_reads >= 128);

    if !likely_unbounded {
        return None;
    }

    Some(SecurityFinding {
        rule_id: "unbounded-iteration".to_string(),
        severity: Severity::High,
        location: "Dynamic trace".to_string(),
        description: format!(
            "Observed high storage-read pressure (reads={}, unique_keys={}, max_reads_single_key={}). This pattern is consistent with unbounded or storage-driven iteration.",
            total_reads,
            unique_keys,
            max_reads_for_one_key
        ),
        remediation: "Use explicit iteration bounds and pagination for storage traversal to avoid gas-denial risks.".to_string(),
        confidence: None,
        rationale: None,
    })
}

fn analyze_reentrancy_pattern_dynamic(trace: &[DynamicTraceEvent]) -> Vec<SecurityFinding> {
    let mut entries = trace.to_vec();
    entries.sort_by_key(|entry| entry.sequence);

    let mut findings = Vec::new();
    let mut writes_seen_by_frame: HashMap<FrameKey, usize> = HashMap::new();
    let mut last_known_frame: Option<FrameKey> = None;
    let mut pending_cross_call: Option<PendingCrossCall> = None;

    for entry in &entries {
        let explicit_frame = frame_key_for(entry);
        let active_frame = explicit_frame.clone().or_else(|| last_known_frame.clone());

        match entry.kind {
            DynamicTraceEventKind::FunctionCall => {
                if let Some(frame) = explicit_frame {
                    last_known_frame = Some(frame);
                }
            }
            DynamicTraceEventKind::StorageWrite => {
                if let Some(frame) = active_frame.clone() {
                    *writes_seen_by_frame.entry(frame.clone()).or_insert(0) += 1;
                    last_known_frame = Some(frame.clone());
                }

                let Some(pending) = pending_cross_call.as_ref() else {
                    continue;
                };

                let same_frame = match (&pending.frame, &active_frame) {
                    (Some(expected), Some(actual)) => expected == actual,
                    _ => false,
                };

                let inferred_match =
                    pending.inferred && pending.frame.is_none() && active_frame.is_none();

                if !(same_frame || inferred_match) {
                    continue;
                }

                if pending.pre_call_write_seen {
                    pending_cross_call = None;
                    continue;
                }

                let (confidence, rationale) = if same_frame {
                    (
                        0.92,
                        format!(
                            "Observed an external interaction at trace event {} and a later \
                             storage write in the same call frame. This matches the classic \
                             checks-effects-interactions violation shape.",
                            pending.sequence
                        ),
                    )
                } else {
                    (
                        0.42,
                        format!(
                            "Observed a global sequence of external call at trace event {} \
                             followed by a storage write, but the trace lacked frame metadata. \
                             Treat this as a low-confidence signal.",
                            pending.sequence
                        ),
                    )
                };

                findings.push(SecurityFinding {
                    rule_id: "reentrancy-pattern".to_string(),
                    severity: if confidence >= 0.8 {
                        Severity::High
                    } else {
                        Severity::Low
                    },
                    location: format!("Trace event {}", entry.sequence),
                    description: "Storage write detected after an external contract call in the same execution frame. Possible reentrancy risk.".to_string(),
                    remediation: "Follow checks-effects-interactions: finalize critical state before external calls, or isolate post-call writes to benign bookkeeping.".to_string(),
                    confidence: Some(confidence),
                    rationale: Some(rationale),
                });
                pending_cross_call = None;
            }
            DynamicTraceEventKind::CrossContractCall => {
                let frame = active_frame.clone();
                let pre_call_write_seen = frame
                    .as_ref()
                    .and_then(|key| writes_seen_by_frame.get(key).copied())
                    .unwrap_or(0)
                    > 0;

                pending_cross_call = Some(PendingCrossCall {
                    frame,
                    sequence: entry.sequence,
                    pre_call_write_seen,
                    inferred: active_frame.is_none(),
                });
            }
            _ => {
                if let Some(frame) = active_frame {
                    last_known_frame = Some(frame);
                }
            }
        }
    }

    findings
}

fn frame_key_for(entry: &DynamicTraceEvent) -> Option<FrameKey> {
    if entry.function.is_none() && entry.call_depth.is_none() {
        return None;
    }

    Some(FrameKey {
        function: entry.function.clone(),
        call_depth: entry.call_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers shared across StrKey tests
    // -----------------------------------------------------------------------

    /// Build a syntactically and cryptographically valid StrKey from raw parts.
    ///
    /// `version` must be `6 << 3` (ED25519 / 'G') or `2 << 3` (contract / 'C').
    /// `key_bytes` must be exactly 32 bytes.
    fn build_strkey(version: u8, key_bytes: &[u8; 32]) -> String {
        // 1. Assemble the 33-byte payload and compute CRC.
        let mut payload = [0u8; 33];
        payload[0] = version;
        payload[1..].copy_from_slice(key_bytes);
        let crc = strkey_crc16(&payload);

        // 2. Concatenate payload + CRC (little-endian) → 35 bytes.
        let mut raw = [0u8; 35];
        raw[..33].copy_from_slice(&payload);
        raw[33..].copy_from_slice(&crc.to_le_bytes());

        // 3. Base32-encode (RFC 4648, A-Z / 2-7).
        const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = String::with_capacity(56);
        let mut bits: u64 = 0;
        let mut bit_count: u32 = 0;
        for &byte in &raw {
            bits = (bits << 8) | (byte as u64);
            bit_count += 8;
            while bit_count >= 5 {
                bit_count -= 5;
                out.push(ALPHABET[((bits >> bit_count) & 0x1f) as usize] as char);
            }
        }
        debug_assert_eq!(out.len(), 56, "StrKey must be exactly 56 chars");
        out
    }

    // -----------------------------------------------------------------------
    // is_valid_strkey — unit tests
    // -----------------------------------------------------------------------

    /// A programmatically constructed StrKey (version 0x30, all-zero key) must
    /// be accepted.  This is the canonical regression guard: if the CRC logic or
    /// base32 decode regresses, this test fails immediately.
    #[test]
    fn strkey_accepts_well_formed_g_address() {
        let addr = build_strkey(6 << 3, &[0u8; 32]);
        assert!(
            addr.starts_with('G'),
            "sanity: version 0x30 encodes to 'G' prefix"
        );
        assert!(
            is_valid_strkey(&addr),
            "well-formed G address must be accepted"
        );
    }

    /// Same for the contract ('C') variant.
    #[test]
    fn strkey_accepts_well_formed_c_address() {
        let addr = build_strkey(2 << 3, &[0u8; 32]);
        assert!(
            addr.starts_with('C'),
            "sanity: version 0x10 encodes to 'C' prefix"
        );
        assert!(
            is_valid_strkey(&addr),
            "well-formed C address must be accepted"
        );
    }

    /// 56 uppercase-ASCII chars starting with 'G' but with all-'A' payload have
    /// the wrong CRC — the rule must NOT fire for them.
    #[test]
    fn strkey_rejects_wrong_checksum() {
        // Build exactly 56 chars: 'G' + 55 'A's.
        // It has a valid prefix/length but an invalid payload+CRC combination.
        let fake = format!("G{}", "A".repeat(55));
        assert_eq!(fake.len(), 56);
        assert!(
            !is_valid_strkey(&fake),
            "all-A token must be rejected (bad CRC)"
        );
    }

    /// A string that is 56 chars, starts with 'G', but contains characters
    /// outside the base32 alphabet (digits 0/1, lower-case letters) must be
    /// rejected before the CRC is even checked.
    #[test]
    fn strkey_rejects_non_base32_characters() {
        // Contains '0', '1', and lower-case letters — all outside A-Z/2-7.
        let bad_chars = "G0001111abcdefghABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDE";
        assert_eq!(bad_chars.len(), 53); // not 56, show next case is the real one
                                         // Craft exactly 56 chars with an invalid char ('0') at position 1.
        let with_zero = "G0AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        assert_eq!(with_zero.len(), 56);
        assert!(
            !is_valid_strkey(with_zero),
            "token with '0' must be rejected"
        );
    }

    /// Strings shorter or longer than 56 characters must always be rejected,
    /// regardless of prefix.
    #[test]
    fn strkey_rejects_wrong_length() {
        assert!(!is_valid_strkey(
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        )); // 55
        assert!(!is_valid_strkey(
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        )); // 57
        assert!(!is_valid_strkey("")); // empty
    }

    /// A valid StrKey with one byte of the checksum flipped must be rejected.
    #[test]
    fn strkey_rejects_flipped_checksum_bit() {
        let good = build_strkey(6 << 3, &[0xab; 32]);
        assert!(is_valid_strkey(&good));

        // Flip the last character to a different (still valid base32) char.
        let mut tampered: Vec<char> = good.chars().collect();
        let last = tampered[55];
        tampered[55] = if last == 'A' { 'B' } else { 'A' };
        let tampered_str: String = tampered.into_iter().collect();
        assert!(
            !is_valid_strkey(&tampered_str),
            "single-char tamper in checksum region must be rejected"
        );
    }

    // -----------------------------------------------------------------------
    // HardcodedAddressRule — fixture tests (the "suggested verification")
    // -----------------------------------------------------------------------

    /// Builds a minimal but structurally valid WASM module whose data section
    /// contains a single string `payload`.  This lets us exercise the full
    /// `analyze_static` path without needing real contract bytes.
    fn wasm_with_data_string(payload: &str) -> Vec<u8> {
        // We embed the payload as a passive data segment.  The encoding is:
        //   magic + version
        //   data-section (id=11):
        //     segment-count = 1
        //     segment: kind=passive(1), byte-length, bytes…
        let data = payload.as_bytes();
        let data_len = data.len();

        // LEB128-encode data_len (works for lengths < 128)
        assert!(data_len < 128, "test helper only handles short payloads");

        let mut wasm = vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
            // Data section (id = 11)
            0x0b,
        ];

        // Section content = segment-count(1) + segment
        // Passive segment: [0x01, data_len_leb, ...bytes...]
        let segment: Vec<u8> = {
            let mut s = vec![0x01, data_len as u8];
            s.extend_from_slice(data);
            s
        };
        let section_content: Vec<u8> = {
            let mut c = vec![0x01]; // segment count = 1
            c.extend_from_slice(&segment);
            c
        };

        wasm.push(section_content.len() as u8); // section length (fits in 1 byte for tests)
        wasm.extend_from_slice(&section_content);
        wasm
    }

    /// **Core regression test** — a data section full of random-looking 56-char
    /// tokens that start with 'G' or 'C' but are NOT valid StrKeys must produce
    /// zero findings.
    #[test]
    fn hardcoded_address_rule_no_finding_for_random_56_char_tokens() {
        // These are 56-char strings starting with 'G'/'C' composed entirely of
        // valid base32 characters, yet none carries a correct CRC-16 checksum.
        let fake_tokens = [
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", // 57 → trim
            "CBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",  // 55 → skip
            // Exactly 56 chars, valid base32, but wrong CRC:
            "GABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRSTUVW",
            "CABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRSTUVW",
        ];

        let rule = HardcodedAddressRule;

        for token in &fake_tokens {
            // Trim/pad to exactly 56 chars for the ones that need it.
            let t: String = token.chars().take(56).collect();
            if t.len() < 56 {
                continue; // shorter than 56 — would not be picked up anyway
            }
            let wasm = wasm_with_data_string(&t);
            let findings = rule
                .analyze_static(&wasm)
                .expect("analyze_static should not error");
            assert!(
                findings.is_empty(),
                "token '{}' must not produce a finding (not a valid StrKey): {:?}",
                t,
                findings
            );
        }
    }

    /// A data section containing a **valid** StrKey must produce exactly one
    /// finding with the correct rule_id.
    #[test]
    fn hardcoded_address_rule_finding_for_valid_strkey() {
        let valid_addr = build_strkey(6 << 3, &[0x42u8; 32]);
        assert_eq!(valid_addr.len(), 56);
        assert!(is_valid_strkey(&valid_addr));

        let wasm = wasm_with_data_string(&valid_addr);
        let rule = HardcodedAddressRule;
        let findings = rule
            .analyze_static(&wasm)
            .expect("analyze_static should not error");

        assert_eq!(
            findings.len(),
            1,
            "exactly one finding expected for a valid hardcoded address"
        );
        assert_eq!(findings[0].rule_id, "hardcoded-address");
        assert!(
            findings[0].description.contains(&valid_addr),
            "finding description must quote the address"
        );
    }

    /// A data section containing **both** valid and invalid tokens must produce a
    /// finding only for the valid StrKey.
    #[test]
    fn hardcoded_address_rule_mixed_tokens() {
        let valid_addr = build_strkey(2 << 3, &[0x11u8; 32]); // C-prefix contract address
                                                              // Pad the two strings with a space so they end up as separate tokens.
        let payload = format!(
            "{} GABCDEFGHIJKLMNOPQRSTUVWXYZ234567ABCDEFGHIJKLMNOPQRSTUV",
            valid_addr
        );

        let wasm = wasm_with_data_string(&payload);
        let rule = HardcodedAddressRule;
        let findings = rule
            .analyze_static(&wasm)
            .expect("analyze_static should not error");

        assert_eq!(
            findings.len(),
            1,
            "only the valid StrKey should be flagged; the garbage token must be ignored"
        );
        assert!(findings[0].description.contains(&valid_addr));
    }

    // -----------------------------------------------------------------------
    // ArithmeticCheckRule / is_guarded — fixture tests
    // -----------------------------------------------------------------------

    /// Bare arithmetic with no surrounding instructions must be flagged.
    #[test]
    fn is_guarded_false_for_isolated_arithmetic() {
        let instrs = vec![WasmInstruction::I32Add];
        assert!(!ArithmeticCheckRule::is_guarded(&instrs, 0));
    }

    /// A `BrIf` immediately *after* the arithmetic is a valid guard.
    #[test]
    fn is_guarded_true_for_brif_after_arithmetic() {
        let instrs = vec![WasmInstruction::I32Add, WasmInstruction::BrIf];
        assert!(ArithmeticCheckRule::is_guarded(&instrs, 0));
    }

    /// An `If` immediately *after* the arithmetic is a valid guard.
    #[test]
    fn is_guarded_true_for_if_after_arithmetic() {
        let instrs = vec![WasmInstruction::I32Add, WasmInstruction::If];
        assert!(ArithmeticCheckRule::is_guarded(&instrs, 0));
    }

    /// A `BrIf` within the 3-instruction lookahead window (with one
    /// intermediate instruction between) is still a valid guard.
    #[test]
    fn is_guarded_true_for_brif_within_lookahead_window() {
        // e.g.: i32.add  ->  i32.const (compare setup)  ->  br_if
        let instrs = vec![
            WasmInstruction::I32Add,
            WasmInstruction::Unknown(0x41),
            WasmInstruction::BrIf,
        ];
        assert!(ArithmeticCheckRule::is_guarded(&instrs, 0));
    }

    /// A `BrIf` that falls *outside* the 3-instruction lookahead must NOT
    /// suppress the finding — the guard is too far away to be meaningful.
    #[test]
    fn is_guarded_false_when_brif_beyond_lookahead() {
        // idx=0, window covers idx+1..idx+4 (indices 1, 2, 3).
        // BrIf is at index 4, which is outside the window.
        let instrs = vec![
            WasmInstruction::I32Add,        // idx 0
            WasmInstruction::Unknown(0x41), // idx 1
            WasmInstruction::Unknown(0x41), // idx 2
            WasmInstruction::Unknown(0x41), // idx 3
            WasmInstruction::BrIf,          // idx 4 — outside window
        ];
        assert!(!ArithmeticCheckRule::is_guarded(&instrs, 0));
    }

    /// **Key regression** — a `BrIf` that appears *before* the arithmetic
    /// (guarding something else entirely) must NOT suppress the finding.
    ///
    /// The old code used `idx.saturating_sub(2)` as the start, so a BrIf
    /// two slots before the arithmetic would incorrectly return true.
    #[test]
    fn is_guarded_false_for_brif_only_before_arithmetic() {
        let instrs = vec![WasmInstruction::BrIf, WasmInstruction::I32Add];
        assert!(!ArithmeticCheckRule::is_guarded(&instrs, 1));
    }

    /// **Key regression** — a `Call` anywhere near the arithmetic must NOT
    /// suppress the finding.  An unrelated call (logger, helper, etc.) is not
    /// a bounds check.
    #[test]
    fn is_guarded_false_for_nearby_unrelated_call() {
        // Call before:
        let before = vec![WasmInstruction::Call, WasmInstruction::I32Add];
        assert!(!ArithmeticCheckRule::is_guarded(&before, 1));

        // Call after:
        let after = vec![WasmInstruction::I32Add, WasmInstruction::Call];
        assert!(!ArithmeticCheckRule::is_guarded(&after, 0));

        // Call on both sides:
        let both = vec![
            WasmInstruction::Call,
            WasmInstruction::I32Mul,
            WasmInstruction::Call,
        ];
        assert!(!ArithmeticCheckRule::is_guarded(&both, 1));
    }

    /// A `Call` between the arithmetic and a `BrIf` must not block the guard
    /// from being recognised — only the presence of If/BrIf matters.
    #[test]
    fn is_guarded_true_when_brif_follows_call_after_arithmetic() {
        // i32.add  ->  call (side-effect)  ->  br_if (checks result)
        let instrs = vec![
            WasmInstruction::I32Add,
            WasmInstruction::Call,
            WasmInstruction::BrIf,
        ];
        assert!(ArithmeticCheckRule::is_guarded(&instrs, 0));
    }

    /// Arithmetic at the very last position of the slice must not panic and
    /// must be reported as unguarded (no instructions ahead to look at).
    #[test]
    fn is_guarded_false_at_end_of_slice() {
        let instrs = vec![WasmInstruction::Unknown(0x41), WasmInstruction::I64Add];
        assert!(!ArithmeticCheckRule::is_guarded(&instrs, 1));
    }

    // -----------------------------------------------------------------------
    // ReentrancyPatternRule — call-frame correlation tests
    // -----------------------------------------------------------------------

    fn make_event(seq: usize, kind: DynamicTraceEventKind, depth: u32) -> DynamicTraceEvent {
        DynamicTraceEvent {
            sequence: seq,
            kind,
            message: String::new(),
            caller: None,
            function: None,
            storage_key: None,
            storage_value: None,
            call_depth: Some(depth as usize),
        }
    }

    /// Safe pattern: cross-contract call at depth 0, storage write happens
    /// inside the callee at depth 1 (different frame) — must produce NO finding.
    #[test]
    fn reentrancy_no_finding_for_write_in_callee_frame() {
        let trace = vec![
            make_event(0, DynamicTraceEventKind::CrossContractCall, 0),
            make_event(1, DynamicTraceEventKind::StorageWrite, 1),
        ];
        assert!(
            analyze_reentrancy_pattern_dynamic(&trace).is_empty(),
            "write in callee frame must not be flagged as reentrancy"
        );
    }

    /// Safe pattern: cross-contract call at depth 0, callee returns (depth drops
    /// back to 0 via a FunctionCall event), then a write at depth 0 in a later
    /// unrelated function — must produce NO finding.
    #[test]
    fn reentrancy_no_finding_for_write_after_call_returned() {
        let trace = vec![
            make_event(0, DynamicTraceEventKind::CrossContractCall, 0),
            make_event(1, DynamicTraceEventKind::StorageWrite, 1),
            make_event(2, DynamicTraceEventKind::CrossContractReturn, 0),
            make_event(3, DynamicTraceEventKind::StorageWrite, 0),
        ];
        assert!(
            analyze_reentrancy_pattern_dynamic(&trace).is_empty(),
            "write after call has returned must not be flagged"
        );
    }

    /// Safe pattern: callee writes at depth 1, then caller writes at depth 0
    /// after an explicit CrossContractReturn — must produce NO finding.
    #[test]
    fn reentrancy_no_finding_for_write_after_callee_write_and_return() {
        let trace = vec![
            make_event(0, DynamicTraceEventKind::CrossContractCall, 0),
            make_event(1, DynamicTraceEventKind::StorageWrite, 1),
            make_event(2, DynamicTraceEventKind::CrossContractReturn, 0),
            make_event(3, DynamicTraceEventKind::StorageWrite, 0),
        ];
        assert!(
            analyze_reentrancy_pattern_dynamic(&trace).is_empty(),
            "write at depth 0 after explicit return must not be flagged"
        );
    }

    /// Unsafe pattern: cross-contract call at depth 0, storage write also at
    /// depth 0 (same frame, after the call) — must produce exactly one finding.
    #[test]
    fn reentrancy_finding_for_write_in_same_frame_after_cross_call() {
        let trace = vec![
            make_event(0, DynamicTraceEventKind::CrossContractCall, 0),
            make_event(1, DynamicTraceEventKind::StorageWrite, 0),
        ];
        let findings = analyze_reentrancy_pattern_dynamic(&trace);
        assert_eq!(
            findings.len(),
            1,
            "write in same frame after cross-contract call must be flagged"
        );
        assert_eq!(findings[0].rule_id, "reentrancy-pattern");
    }

    // Pre-existing tests (unchanged)

    #[test]
    fn unbounded_iteration_dynamic_flags_high_risk_pattern() {
        let mut trace = Vec::new();
        for i in 0..90usize {
            trace.push(DynamicTraceEvent {
                sequence: i,
                kind: DynamicTraceEventKind::StorageRead,
                message: "contract_storage_get".to_string(),
                caller: None,
                function: Some("sweep".to_string()),
                call_depth: Some(0),
                storage_key: Some(format!("user:{}", i % 4)),
                storage_value: None,
            });
        }

        let finding = analyze_unbounded_iteration_dynamic(&trace);
        assert!(finding.is_some());
        assert!(matches!(finding.unwrap().severity, Severity::High));
    }

    #[test]
    fn static_signal_false_for_non_wasm_bytes() {
        let signal = analyze_unbounded_iteration_static(&[1, 2, 3, 4, 5]);
        assert!(!signal.suspicious);
    }

    #[test]
    fn reentrancy_rule_flags_same_frame_write_after_cross_contract_call() {
        let findings = analyze_reentrancy_pattern_dynamic(&[
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::FunctionCall,
                message: "main -> withdraw".to_string(),
                caller: Some("main".to_string()),
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::CrossContractCall,
                message: "withdraw invokes token.transfer".to_string(),
                caller: Some("main".to_string()),
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
            },
            DynamicTraceEvent {
                sequence: 3,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write balance".to_string(),
                caller: Some("main".to_string()),
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: Some("balance:alice".to_string()),
                storage_value: Some("0".to_string()),
            },
        ]);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "reentrancy-pattern");
        assert!(matches!(findings[0].severity, Severity::High));
        assert!(findings[0].confidence.unwrap_or_default() >= 0.8);
        assert!(findings[0]
            .rationale
            .as_deref()
            .unwrap_or_default()
            .contains("same call frame"));
    }

    #[test]
    fn reentrancy_rule_skips_post_call_write_when_pre_call_effect_seen_in_same_frame() {
        let findings = analyze_reentrancy_pattern_dynamic(&[
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::FunctionCall,
                message: "main -> settle".to_string(),
                caller: Some("main".to_string()),
                function: Some("settle".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "mark settled".to_string(),
                caller: Some("main".to_string()),
                function: Some("settle".to_string()),
                call_depth: Some(0),
                storage_key: Some("settled:alice".to_string()),
                storage_value: Some("true".to_string()),
            },
            DynamicTraceEvent {
                sequence: 3,
                kind: DynamicTraceEventKind::CrossContractCall,
                message: "settle invokes payout".to_string(),
                caller: Some("main".to_string()),
                function: Some("settle".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
            },
            DynamicTraceEvent {
                sequence: 4,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "emit bookkeeping marker".to_string(),
                caller: Some("main".to_string()),
                function: Some("settle".to_string()),
                call_depth: Some(0),
                storage_key: Some("audit:last_settle".to_string()),
                storage_value: Some("1".to_string()),
            },
        ]);

        assert!(findings.is_empty());
    }

    #[test]
    fn reentrancy_rule_skips_write_in_different_frame_after_cross_contract_call() {
        let findings = analyze_reentrancy_pattern_dynamic(&[
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::FunctionCall,
                message: "main -> withdraw".to_string(),
                caller: Some("main".to_string()),
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::CrossContractCall,
                message: "withdraw invokes token.transfer".to_string(),
                caller: Some("main".to_string()),
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
            },
            DynamicTraceEvent {
                sequence: 3,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "nested contract writes receipt".to_string(),
                caller: Some("withdraw".to_string()),
                function: Some("token.transfer".to_string()),
                call_depth: Some(1),
                storage_key: Some("receipt:1".to_string()),
                storage_value: Some("ok".to_string()),
            },
        ]);

        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // is_storage_read_import — variant name and module matching tests
    // -----------------------------------------------------------------------

    #[test]
    fn storage_read_import_detects_known_variants() {
        let cases = [
            ("env", "storage_get"),
            ("env", "storage_has"),
            ("env", "storage_next"),
            ("env", "storage_iter"),
            ("env", "get_contract_data"),
            ("env", "has_contract_data"),
            ("env", "map_get"),
            ("env", "vec_get"),
            ("soroban_env", "storage_get"),
            ("soroban-env-host", "storage_get_v2"),
            ("soroban_env_host", "get_contract_data_v3"),
        ];
        for (module, name) in cases {
            assert!(
                is_storage_read_import(module, name),
                "expected is_storage_read_import to match {module}::{name}"
            );
        }
    }

    #[test]
    fn storage_read_import_ignores_unrelated_names() {
        assert!(!is_storage_read_import("env", "reinvoke_storage_getter"));
        assert!(!is_storage_read_import("env", "storage_put"));
        assert!(!is_storage_read_import("env", "log_get"));
        assert!(!is_storage_read_import("env", "invoke_contract"));
    }

    #[test]
    fn storage_read_import_ignores_unrelated_modules() {
        assert!(!is_storage_read_import("not_env", "storage_get"));
        assert!(!is_storage_read_import("mylib", "storage_get"));
        assert!(!is_storage_read_import("environments", "storage_get"));
    }

    // -----------------------------------------------------------------------
    // AuthorizationCheckRule — dynamic trace tests
    // -----------------------------------------------------------------------

    #[test]
    fn auth_rule_detects_storage_before_auth() {
        let rule = AuthorizationCheckRule;

        // Test case: storage write happens before authorization
        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                call_depth: Some(0),
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: None,
                storage_value: None,
                call_depth: Some(0),
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("1 storage write(s) occurring before any authorization event"));
    }

    #[test]
    fn auth_rule_allows_storage_after_auth() {
        let rule = AuthorizationCheckRule;

        // Test case: authorization happens before storage write (should be OK)
        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: None,
                storage_value: None,
                call_depth: Some(0),
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                call_depth: Some(0),
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn auth_rule_detects_multiple_storage_before_auth() {
        let rule = AuthorizationCheckRule;

        // Test case: multiple storage writes happen before authorization
        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                call_depth: Some(0),
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key2".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: Some("key2".to_string()),
                storage_value: Some("value2".to_string()),
                call_depth: Some(0),
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: None,
                storage_value: None,
                call_depth: Some(0),
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("2 storage write(s) occurring before any authorization event"));
    }

    #[test]
    fn auth_rule_detects_storage_without_any_auth() {
        let rule = AuthorizationCheckRule;

        // Test case: storage writes with no authorization at all
        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                call_depth: Some(0),
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key2".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                storage_key: Some("key2".to_string()),
                storage_value: Some("value2".to_string()),
                call_depth: Some(0),
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("2 storage write(s) occurring before any authorization event"));
    }
}
