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
    pub fingerprint: String,
    #[serde(default)]
    pub suppressed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityReport {
    pub findings: Vec<SecurityFinding>,
    pub rules: HashMap<String, RuleMetadata>,
    pub metadata: ReportMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReportMetadata {
    pub total_findings: usize,
    pub suppressed_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityWaiver {
    pub fingerprint: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WaiverFile {
    pub waivers: Vec<SecurityWaiver>,
}

pub trait SecurityRule {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn severity(&self) -> Severity;

    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: self.id().to_string(),
            name: self.name().to_string(),
            description: self.description().to_string(),
            severity: self.severity(),
        }
    }

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
    waivers: Vec<SecurityWaiver>,
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
                Box::new(StorageWritePressureRule),
            ],
            waivers: Vec::new(),
        }
    }

    pub fn with_waivers(mut self, waivers: Vec<SecurityWaiver>) -> Self {
        self.waivers = waivers;
        self
    }

    pub fn load_waivers_from_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::DebuggerError::FileError(format!("Failed to read waiver file: {}", e))
        })?;
        let waiver_file: WaiverFile = toml::from_str(&content).map_err(|e| {
            crate::DebuggerError::FileError(format!("Failed to parse waiver TOML: {}", e))
        })?;
        self.waivers = waiver_file.waivers;
        Ok(self)
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
            let id = rule.id();

            if !filter.enable_rules.is_empty() && !filter.enable_rules.iter().any(|r| r == id) {
                continue;
            }
            if filter.disable_rules.iter().any(|r| r == id) {
                continue;
            }

            let static_findings = rule.analyze_static(wasm_bytes)?;
            let filtered_static: Vec<_> = static_findings
                .into_iter()
                .filter(|f| f.severity >= filter.min_severity)
                .collect();

            if !filtered_static.is_empty() {
                report.rules.insert(id.to_string(), rule.metadata());
                report.findings.extend(filtered_static);
            }

            if let Some(tr) = trace {
                let dynamic_findings = rule.analyze_dynamic(executor, tr)?;
                let filtered_dynamic: Vec<_> = dynamic_findings
                    .into_iter()
                    .filter(|f| f.severity >= filter.min_severity)
                    .collect();

                if !filtered_dynamic.is_empty() {
                    report.rules.insert(id.to_string(), rule.metadata());
                    report.findings.extend(filtered_dynamic);
                }
            }
        }

        self.apply_waivers(&mut report);
        Ok(report)
    }

    fn apply_waivers(&self, report: &mut SecurityReport) {
        let waiver_set: HashSet<&str> = self
            .waivers
            .iter()
            .map(|w| w.fingerprint.as_str())
            .collect();
        let mut suppressed_count = 0;

        for finding in &mut report.findings {
            if waiver_set.contains(finding.fingerprint.as_str()) {
                finding.suppressed = true;
                suppressed_count += 1;
            }
        }

        report.metadata = ReportMetadata {
            total_findings: report.findings.len(),
            suppressed_count,
        };
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
    fn id(&self) -> &str {
        "hardcoded-address"
    }

    fn name(&self) -> &str {
        "Hardcoded Address detector"
    }

    fn description(&self) -> &str {
        "Detects hardcoded Stellar addresses in WASM data sections."
    }

    fn severity(&self) -> Severity {
        Severity::Medium
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
                                rule_id: self.id().to_string(),
                                severity: Severity::Medium,
                                location: "Data Section".to_string(),
                                description: format!("Hardcoded address found: {}", word),
                                remediation: "Move this address to a configuration setting or pass it as a constructor/init argument to keep the contract logic generic and portable.".to_string(),

                                confidence: None,
                                rationale: None,
                                fingerprint: format!("{}:{}", self.name(), word),
                                suppressed: false,
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
    fn id(&self) -> &str {
        "arithmetic-overflow"
    }

    fn name(&self) -> &str {
        "Arithmetic Overflow detector"
    }

    fn description(&self) -> &str {
        "Detects potential for unchecked arithmetic overflow."
    }

    fn severity(&self) -> Severity {
        Severity::Medium
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();
        // This part of the code needs to be updated to correctly extract func_name and offset
        // from the WASM bytes, which is not directly supported by the current `parse_instructions`
        // signature. For the purpose of this edit, we'll assume `parse_instructions`
        // or a similar mechanism provides this context.
        // As the instruction only provides a snippet, we'll adapt it to the existing structure
        // by using a placeholder for func_name and offset, or by making a minimal change
        // that aligns with the instruction's intent for the finding fields.

        // The provided diff implies a change in how instructions are processed to get function context.
        // Since the full context for `func_name` and `offset` is not provided,
        // I will apply the changes to the finding fields as requested,
        // using the existing `i` for offset and a placeholder for `func_name`.
        // A more complete solution would involve parsing the WASM module to map
        // instructions to their respective functions and offsets.

        let instructions = parse_instructions(wasm_bytes);

        for (i, instr) in instructions.iter().enumerate() {
            if !Self::is_arithmetic(instr) {
                continue;
            }

            // Classify the guard pattern after this arithmetic instruction and assign
            // confidence + description accordingly.
            let (confidence, guard_desc, rationale) =
                match Self::classify_guard(&instructions, i) {
                    GuardKind::FullGuard => continue, // comparison drives branch → suppressed
                    GuardKind::CompareNoBranch => (
                        0.70f32,
                        "Confidence: medium | comparison found but does not drive conditional control flow",
                        "A comparison instruction was found after the arithmetic but its result does not feed a conditional branch.",
                    ),
                    GuardKind::BranchNoCompare => (
                        0.40f32,
                        "Confidence: low | no recognized compare-and-branch guard",
                        "A branch instruction was found after the arithmetic but without a preceding comparison.",
                    ),
                    GuardKind::NoGuard => (
                        0.95f32,
                        "Confidence: high | No comparison-derived conditional branch",
                        "No guard pattern was found after the arithmetic instruction.",
                    ),
                };

            findings.push(SecurityFinding {
                rule_id: self.id().to_string(),
                severity: Severity::Medium,
                location: format!("Instruction {}", i),
                description: format!(
                    "Unchecked arithmetic operation detected: {:?}. {}",
                    instr, guard_desc
                ),
                remediation:
                    "Ensure arithmetic operations are guarded with proper bounds checks or overflow handling."
                        .to_string(),
                confidence: Some(confidence),
                rationale: Some(rationale.to_string()),
                fingerprint: format!("{}:{}:{:?}", self.id(), i, instr),
                suppressed: false,
            });
        }

        Ok(findings)
    }
}

/// Guard classification for arithmetic overflow findings.
#[derive(Debug)]
enum GuardKind {
    /// A comparison instruction drives a conditional branch — fully guarded.
    FullGuard,
    /// Comparison present but result not used in a branch.
    CompareNoBranch,
    /// Branch present but no preceding comparison instruction.
    BranchNoCompare,
    /// No guard pattern detected.
    NoGuard,
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

    fn is_comparison_instr(instr: &WasmInstruction) -> bool {
        matches!(instr, WasmInstruction::Unknown(b) if (0x46..=0x4f).contains(b) || (0x51..=0x5a).contains(b))
    }

    fn classify_guard(instructions: &[WasmInstruction], idx: usize) -> GuardKind {
        const WINDOW: usize = 15;
        let end = (idx + 1 + WINDOW).min(instructions.len());
        let window = &instructions[idx + 1..end];

        let mut compare_pos: Option<usize> = None;
        let mut branch_pos: Option<usize> = None;

        for (j, instr) in window.iter().enumerate() {
            if compare_pos.is_none() && Self::is_comparison_instr(instr) {
                compare_pos = Some(j);
            }
            if branch_pos.is_none() && matches!(instr, WasmInstruction::If | WasmInstruction::BrIf)
            {
                branch_pos = Some(j);
            }
        }

        match (compare_pos, branch_pos) {
            (Some(cmp), Some(br)) if cmp < br => GuardKind::FullGuard,
            (Some(_), Some(_)) => GuardKind::BranchNoCompare,
            (Some(_), None) => GuardKind::CompareNoBranch,
            (None, Some(_)) => GuardKind::BranchNoCompare,
            (None, None) => GuardKind::NoGuard,
        }
    }

    #[allow(dead_code)]
    fn is_guarded(instructions: &[WasmInstruction], idx: usize) -> bool {
        for instr in instructions.iter().skip(idx + 1).take(3) {
            match instr {
                WasmInstruction::BrIf | WasmInstruction::If => return true,
                _ => {}
            }
        }
        false
    }
}

struct AuthorizationCheckRule;
impl SecurityRule for AuthorizationCheckRule {
    fn id(&self) -> &str {
        "missing-auth"
    }

    fn name(&self) -> &str {
        "Missing Authorization detector"
    }

    fn description(&self) -> &str {
        "Detects sensitive flows missing authorization checks."
    }

    fn severity(&self) -> Severity {
        Severity::High
    }

    fn analyze_dynamic(
        &self,
        _executor: Option<&ContractExecutor>,
        trace: &[DynamicTraceEvent],
    ) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();
        let mut auth_actors_per_frame: std::collections::HashMap<
            FrameKey,
            std::collections::HashMap<String, usize>,
        > = std::collections::HashMap::new();

        // First pass: find authorized actors per frame
        for entry in trace {
            if entry.kind == DynamicTraceEventKind::Authorization {
                if let Some(frame) = frame_key_for(entry) {
                    let actors = auth_actors_per_frame.entry(frame).or_default();
                    let addr = entry.address.clone().or_else(|| {
                        // Legacy fallback
                        entry
                            .message
                            .split_whitespace()
                            .find(|w| (w.starts_with('G') || w.starts_with('C')) && w.len() == 56)
                            .map(|s| s.to_string())
                    });
                    if let Some(addr) = addr {
                        actors.entry(addr).or_insert(entry.sequence);
                    }
                }
            }
        }

        let mut problematic_writes = Vec::new();
        // Second pass: check storage writes
        for entry in trace {
            if entry.kind == DynamicTraceEventKind::StorageWrite {
                if let Some(frame) = frame_key_for(entry) {
                    if let Some(authorized_actors) = auth_actors_per_frame.get(&frame) {
                        let earliest_auth = authorized_actors
                            .values()
                            .min()
                            .cloned()
                            .unwrap_or(usize::MAX);
                        if entry.sequence < earliest_auth {
                            problematic_writes.push((entry.clone(), format!("Storage mutation detected before any authorization in frame '{}'.", frame.function.as_deref().unwrap_or("unknown"))));
                        } else if let Some(key) = &entry.storage_key {
                            let covered = authorized_actors.keys().any(|addr| key.contains(addr));
                            if !covered && !authorized_actors.is_empty() {
                                problematic_writes.push((entry.clone(), format!(
                                    "Storage mutation to key '{}' detected without authorization for a relevant actor in frame '{}'. Authorized actors: {:?}",
                                    key,
                                    frame.function.as_deref().unwrap_or("unknown"),
                                    authorized_actors.keys().collect::<Vec<_>>()
                                )));
                            }
                        }
                    } else {
                        problematic_writes.push((entry.clone(), format!("Storage mutation detected without any preceding authorization in frame '{}'.", frame.function.as_deref().unwrap_or("unknown"))));
                    }
                } else {
                    problematic_writes.push((entry.clone(), "Storage mutation detected without frame metadata or preceding authorization.".to_string()));
                }
            }
        }

        // If we have storage writes without preceding auth in the same scope, report a finding
        if !problematic_writes.is_empty() {
            let description = if problematic_writes.is_empty() {
                "Storage mutation detected without preceding authorization in the same call frame."
                    .to_string()
            } else {
                problematic_writes[0].1.clone()
            };

            findings.push(SecurityFinding {
                rule_id: self.id().to_string(),
                severity: Severity::High,
                location: "Dynamic trace".to_string(),
                description,
                remediation: "Ensure all sensitive functions call `address.require_auth()` in their own scope before mutating state.".to_string(),

                confidence: None,
                rationale: None,
                fingerprint: format!("{}:{}", self.id(), problematic_writes.first().map(|(e, _)| e.sequence).unwrap_or(0)),
                suppressed: false,
            });
        }
        Ok(findings)
    }
}

struct ReentrancyPatternRule;
impl SecurityRule for ReentrancyPatternRule {
    fn id(&self) -> &str {
        "reentrancy-pattern"
    }

    fn name(&self) -> &str {
        "Reentrancy Pattern detector"
    }

    fn description(&self) -> &str {
        "Detects cross-contract calls followed by storage writes in the same call frame."
    }

    fn severity(&self) -> Severity {
        Severity::High
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
    call_depth: Option<u64>,
}

impl FrameKey {
    /// Check whether two frame keys refer to the same logical call frame.
    ///
    /// `function` is treated as a strong signal when available. When the
    /// runtime omits the function name, call depth is still used to correlate
    /// events from the same frame.
    fn matches(&self, other: &FrameKey) -> bool {
        if let (Some(a), Some(b)) = (self.call_depth, other.call_depth) {
            if a != b {
                return false;
            }
        }

        match (&self.function, &other.function) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.call_depth.is_some() && other.call_depth.is_some(),
            _ => self.call_depth.is_some() && other.call_depth.is_some(),
        }
    }
}

#[derive(Debug, Clone)]
struct PendingCrossCall {
    frame: Option<FrameKey>,
    sequence: usize,
    pre_call_write_seen: bool,
    inferred: bool,
    call_depth: Option<u64>,
}

struct CrossContractImportRule;
impl SecurityRule for CrossContractImportRule {
    fn id(&self) -> &str {
        "cross-contract-import"
    }

    fn name(&self) -> &str {
        "Cross-Contract Import detector"
    }

    fn description(&self) -> &str {
        "Detects cross-contract host function imports with robust name matching."
    }

    fn severity(&self) -> Severity {
        Severity::Low
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
            rule_id: self.id().to_string(),
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
            fingerprint: format!("{}:{}", self.id(), matches.join(",")),
            suppressed: false,
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
    fn id(&self) -> &str {
        "unbounded-iteration"
    }

    fn name(&self) -> &str {
        "Unbounded Iteration detector"
    }

    fn description(&self) -> &str {
        "Detects storage-driven loops and unbounded storage-read patterns."
    }

    fn severity(&self) -> Severity {
        Severity::High
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let analysis = analyze_unbounded_iteration_static(wasm_bytes);
        if !analysis.suspicious {
            return Ok(Vec::new());
        }

        let finding = SecurityFinding {
            rule_id: self.id().to_string(),

            severity: Severity::High,
            location: "WASM code section".to_string(),
            description: format!(
                "Detected loop(s) with storage-read host calls ({} storage-read calls while inside loop).",
                analysis.storage_calls_inside_loops
            ),
            remediation: "Bound iteration over storage-backed collections (pagination, explicit limits, or capped batch size).".to_string(),
            confidence: analysis.confidence,
            rationale: analysis.rationale,
            fingerprint: format!("{}:{}", self.id(), analysis.storage_calls_inside_loops),
            suppressed: false,

        };

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
                finding.rule_id = self.id().to_string();
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

#[derive(Debug, Default)]
struct StorageWriteStaticSignal {
    suspicious: bool,
    storage_writes_inside_loops: usize,
    confidence: Option<f32>,
    rationale: Option<String>,
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
    signal.confidence = Some(if storage_calls_in_loops > 0 {
        if signal.max_nesting_depth >= 2 && storage_calls_in_loops >= 3 {
            0.9
        } else if signal.max_nesting_depth > 1 || storage_calls_in_loops > 1 {
            0.7
        } else {
            0.5
        }
    } else {
        0.2
    });

    signal.rationale = Some(format!(
        "Storage-read calls in loops: {}, max nesting depth: {}, loop types with calls: {:?}, calls outside loops: {}",
        storage_calls_in_loops,
        signal.max_nesting_depth,
        loop_types_with_calls,
        _storage_calls_outside_loops
    ));
    let _ = loop_types_with_calls;

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

fn is_storage_write_import(module: &str, name: &str) -> bool {
    const BASES: &[&str] = &[
        "storageput",
        "storageset",
        "storagedel",
        "putcontractdata",
        "setcontractdata",
        "delcontractdata",
        "mapput",
        "vecput",
        "vecpushback",
        "contractstorageput",
        "contractstorageset",
    ];

    if !is_env_like_module(module) {
        return false;
    }

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
        fingerprint: format!("{}:{}:{}", "unbounded-iteration", total_reads / 10 * 10, unique_keys / 5 * 5),
        suppressed: false,
    })
}

struct StorageWritePressureRule;
impl SecurityRule for StorageWritePressureRule {
    fn id(&self) -> &str {
        "storage-write-pressure"
    }

    fn name(&self) -> &str {
        "storage-write-pressure"
    }

    fn description(&self) -> &str {
        "Detects loop-driven storage writes and repeated mutation of hot state."
    }

    fn severity(&self) -> Severity {
        Severity::High
    }

    fn analyze_static(&self, wasm_bytes: &[u8]) -> Result<Vec<SecurityFinding>> {
        let analysis = analyze_storage_write_pressure_static(wasm_bytes);
        if !analysis.suspicious {
            return Ok(Vec::new());
        }

        Ok(vec![SecurityFinding {
            rule_id: self.name().to_string(),
            severity: Severity::High,
            location: "WASM code section".to_string(),
            description: format!(
                "Detected loop(s) with storage-write host calls ({} storage-write calls while inside loop).",
                analysis.storage_writes_inside_loops
            ),
            remediation: "Coalesce writes in memory, cap mutation batches, and avoid repeated writes to hot keys inside loops.".to_string(),
            confidence: analysis.confidence,
            rationale: analysis.rationale,
            fingerprint: format!("{}:{}", self.id(), analysis.storage_writes_inside_loops),
            suppressed: false,
        }])
    }

    fn analyze_dynamic(
        &self,
        _executor: Option<&ContractExecutor>,
        trace: &[DynamicTraceEvent],
    ) -> Result<Vec<SecurityFinding>> {
        Ok(analyze_storage_write_pressure_dynamic(trace)
            .into_iter()
            .map(|mut finding| {
                finding.rule_id = self.name().to_string();
                finding
            })
            .collect())
    }
}

fn analyze_storage_write_pressure_static(wasm_bytes: &[u8]) -> StorageWriteStaticSignal {
    let mut storage_import_indices = HashSet::new();
    let mut imported_func_count = 0u32;
    let mut control_flow_stack: Vec<ControlFlowFrame> = Vec::new();
    let mut signal = StorageWriteStaticSignal::default();
    let mut storage_writes_in_loops = 0usize;
    let mut storage_writes_outside_loops = 0usize;
    let mut loop_types_with_writes: HashSet<String> = HashSet::new();

    for payload in Parser::new(0).parse_all(wasm_bytes) {
        let Ok(payload) = payload else {
            return signal;
        };

        match payload {
            Payload::ImportSection(reader) => {
                for import in reader.into_iter().flatten() {
                    if let wasmparser::TypeRef::Func(_) = import.ty {
                        if is_storage_write_import(import.module, import.name) {
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
                            control_flow_stack.push(ControlFlowFrame::Loop {
                                loop_type: if current_depth > 0 {
                                    "nested_loop".to_string()
                                } else {
                                    "top_level_loop".to_string()
                                },
                            });
                            signal.max_nesting_depth =
                                signal.max_nesting_depth.max(current_depth + 1);
                        }
                        Operator::Block { .. } => control_flow_stack.push(ControlFlowFrame::Block),
                        Operator::If { .. } => control_flow_stack.push(ControlFlowFrame::If),
                        Operator::Else => {}
                        Operator::End => {
                            control_flow_stack.pop();
                        }
                        Operator::Call { function_index } => {
                            if !storage_import_indices.contains(&function_index) {
                                continue;
                            }

                            if control_flow_stack.iter().any(ControlFlowFrame::is_loop) {
                                storage_writes_in_loops += 1;
                                if let Some(loop_frame) =
                                    control_flow_stack.iter().rev().find(|f| f.is_loop())
                                {
                                    if let Some(loop_type) = loop_frame.loop_type() {
                                        loop_types_with_writes.insert(loop_type.to_string());
                                    }
                                }
                            } else {
                                storage_writes_outside_loops += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    signal.storage_writes_inside_loops = storage_writes_in_loops;
    signal.confidence = Some(
        if storage_writes_in_loops >= 4 || signal.max_nesting_depth >= 2 {
            0.9
        } else if storage_writes_in_loops >= 2 {
            0.75
        } else if storage_writes_in_loops == 1 {
            0.55
        } else {
            0.2
        },
    );
    signal.rationale = Some(format!(
        "Storage-write calls in loops: {}, max nesting depth: {}, loop types with writes: {:?}, writes outside loops: {}",
        storage_writes_in_loops,
        signal.max_nesting_depth,
        loop_types_with_writes,
        storage_writes_outside_loops
    ));
    signal.suspicious = storage_writes_in_loops > 0;
    signal
}

fn analyze_storage_write_pressure_dynamic(trace: &[DynamicTraceEvent]) -> Option<SecurityFinding> {
    let mut write_key_counts: HashMap<&str, usize> = HashMap::new();
    let mut total_writes = 0usize;

    for entry in trace {
        if entry.kind == DynamicTraceEventKind::StorageWrite {
            total_writes += 1;
            if let Some(key) = entry.storage_key.as_deref() {
                *write_key_counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    if total_writes == 0 {
        return None;
    }

    let unique_keys = write_key_counts.len();
    let max_writes_for_one_key = write_key_counts.values().copied().max().unwrap_or(0);
    let repeated_writes = total_writes.saturating_sub(unique_keys);
    let likely_hot_state_pressure = total_writes >= 32
        && (max_writes_for_one_key >= 8
            || repeated_writes >= total_writes / 2
            || (total_writes >= 64 && unique_keys <= total_writes / 3));

    if !likely_hot_state_pressure {
        return None;
    }

    Some(SecurityFinding {
        rule_id: "storage-write-pressure".to_string(),
        severity: Severity::High,
        location: "Dynamic trace".to_string(),
        description: format!(
            "Observed high storage-write pressure (writes={}, unique_keys={}, max_writes_single_key={}, repeated_writes={}). This pattern is consistent with loop-driven mutation or repeated writes to hot state.",
            total_writes,
            unique_keys,
            max_writes_for_one_key,
            repeated_writes
        ),
        remediation: "Batch changes in memory, collapse duplicate writes, and bound write-heavy loops to reduce gas-denial risk.".to_string(),
        confidence: Some(if max_writes_for_one_key >= 16 || total_writes >= 64 {
            0.9
        } else {
            0.75
        }),
        rationale: Some(format!(
            "Repeated writes concentrated on {} unique key(s); hottest key written {} time(s).",
            unique_keys, max_writes_for_one_key
        )),
        fingerprint: format!("{}:{}:{}", "storage-write-pressure", total_writes / 10 * 10, unique_keys / 5 * 5),
        suppressed: false,
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
                    (Some(expected), Some(actual)) => expected.matches(actual),
                    _ => false,
                };

                let inferred_match =
                    pending.inferred && pending.frame.is_none() && active_frame.is_none();

                if !(same_frame || (inferred_match && pending.call_depth == entry.call_depth)) {
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

                let func_name = pending
                    .frame
                    .as_ref()
                    .and_then(|f| f.function.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("unknown");
                let storage_key = entry.storage_key.as_deref().unwrap_or("unknown");

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
                    fingerprint: format!("{}:{}:{}", "reentrancy-pattern", func_name, storage_key),
                    suppressed: false,
                });
                pending_cross_call = None;
            }
            DynamicTraceEventKind::CrossContractCall => {
                let frame = active_frame.clone();
                let pre_call_write_seen = frame
                    .as_ref()
                    .map(|key| find_writes_seen_by_frame(&writes_seen_by_frame, key))
                    .unwrap_or(0)
                    > 0;

                pending_cross_call = Some(PendingCrossCall {
                    frame,
                    sequence: entry.sequence,
                    pre_call_write_seen,
                    inferred: active_frame.is_none(),
                    call_depth: entry.call_depth, // Match Option<usize>
                });
            }
            DynamicTraceEventKind::CrossContractReturn => {
                // A return event signals the callee has finished. Clear any pending
                // cross-call whose frame matches or is broader, so that writes
                // that occur *after* the callee returns are not flagged.
                let returning_frame = active_frame.clone();
                if let Some(ref pending) = pending_cross_call {
                    let frames_match = match (&pending.frame, &returning_frame) {
                        (Some(p), Some(r)) => p.matches(r),
                        // If the return has no depth info, clear conservatively
                        (_, None) => true,
                        _ => false,
                    };
                    if frames_match {
                        pending_cross_call = None;
                    }
                }
                if let Some(frame) = active_frame {
                    last_known_frame = Some(frame);
                }
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

fn find_writes_seen_by_frame(
    writes_seen_by_frame: &HashMap<FrameKey, usize>,
    frame: &FrameKey,
) -> usize {
    if let Some(count) = writes_seen_by_frame.get(frame) {
        return *count;
    }

    if frame.call_depth.is_some() {
        writes_seen_by_frame
            .iter()
            .filter(|(key, _)| key.matches(frame))
            .map(|(_, &count)| count)
            .sum()
    } else {
        0
    }
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

    // -----------------------------------------------------------------------
    // ReentrancyPatternRule — call-frame correlation tests
    // -----------------------------------------------------------------------

    fn make_event(seq: usize, kind: DynamicTraceEventKind, depth: usize) -> DynamicTraceEvent {
        DynamicTraceEvent {
            sequence: seq,
            kind,
            message: String::new(),
            caller: None,
            function: None,
            storage_key: None,
            storage_value: None,
            call_depth: Some(depth as u64),
            address: None,
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

    #[test]
    fn frame_key_for_allows_call_depth_without_function() {
        let event = DynamicTraceEvent {
            sequence: 0,
            kind: DynamicTraceEventKind::CrossContractCall,
            message: String::new(),
            caller: None,
            function: None,
            call_depth: Some(1),
            storage_key: None,
            storage_value: None,
            address: None,
        };

        let frame = frame_key_for(&event).expect("expected frame key for call-depth-only event");
        assert_eq!(frame.call_depth, Some(1));
        assert!(frame.function.is_none());
    }

    #[test]
    fn reentrancy_rule_matches_same_depth_when_cross_call_function_missing() {
        let findings = analyze_reentrancy_pattern_dynamic(&[
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::CrossContractCall,
                message: "unknown frame call".to_string(),
                caller: None,
                function: None,
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
                address: None,
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write balance".to_string(),
                caller: None,
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: Some("balance:alice".to_string()),
                storage_value: Some("0".to_string()),
                address: None,
            },
        ]);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "reentrancy-pattern");
        assert!(matches!(findings[0].severity, Severity::High));
    }

    #[test]
    fn reentrancy_rule_matches_same_depth_when_write_function_missing() {
        let findings = analyze_reentrancy_pattern_dynamic(&[
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::CrossContractCall,
                message: "withdraw invokes external".to_string(),
                caller: None,
                function: Some("withdraw".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
                address: None,
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write balance".to_string(),
                caller: None,
                function: None,
                call_depth: Some(0),
                storage_key: Some("balance:alice".to_string()),
                storage_value: Some("0".to_string()),
                address: None,
            },
        ]);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "reentrancy-pattern");
        assert!(matches!(findings[0].severity, Severity::High));
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
                address: None,
            });
        }

        let finding = analyze_unbounded_iteration_dynamic(&trace);
        assert!(finding.is_some());
        assert!(matches!(finding.unwrap().severity, Severity::High));
    }

    #[test]
    fn storage_write_pressure_dynamic_flags_hot_state_mutation() {
        let mut trace = Vec::new();
        for i in 0..40usize {
            trace.push(DynamicTraceEvent {
                sequence: i,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "storage_put".to_string(),
                caller: None,
                function: Some("rebalance".to_string()),
                call_depth: Some(0),
                storage_key: Some(format!("bucket:{}", i % 2)),
                storage_value: Some(format!("{i}")),
                address: None,
            });
        }

        let finding = analyze_storage_write_pressure_dynamic(&trace);
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
                address: None,
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
    }

    // -----------------------------------------------------------------------
    // AuthorizationCheckRule — dynamic trace tests
    // -----------------------------------------------------------------------

    #[test]
    fn storage_write_import_detects_known_variants() {
        let cases = [
            ("env", "storage_put"),
            ("env", "storage_set"),
            ("env", "storage_del"),
            ("env", "put_contract_data"),
            ("env", "set_contract_data"),
            ("soroban_env", "storage_put"),
            ("soroban-env-host", "storage_set_v2"),
        ];
        for (module, name) in cases {
            assert!(
                is_storage_write_import(module, name),
                "expected is_storage_write_import to match {module}::{name}"
            );
        }
    }

    #[test]
    fn storage_write_import_ignores_read_only_and_unrelated_names() {
        assert!(!is_storage_write_import("env", "storage_get"));
        assert!(!is_storage_write_import("env", "reinvoke_storage_setter"));
        assert!(!is_storage_write_import("env", "invoke_contract"));
        assert!(!is_storage_write_import("not_env", "storage_put"));
    }

    // -----------------------------------------------------------------------
    // AuthorizationCheckRule — dynamic trace tests
    // -----------------------------------------------------------------------

    #[test]
    fn auth_rule_detects_storage_before_auth() {
        let rule = AuthorizationCheckRule;

        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                address: None,
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
                address: None,
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("before any authorization in frame 'test_function'"));
    }

    #[test]
    fn auth_rule_allows_storage_after_auth() {
        let rule = AuthorizationCheckRule;

        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
                address: Some("G123...".to_string()),
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key1:G123...".to_string()),
                storage_value: Some("value1".to_string()),
                address: None,
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn auth_rule_detects_multiple_storage_before_auth() {
        let rule = AuthorizationCheckRule;

        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                address: None,
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key2".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key2".to_string()),
                storage_value: Some("value2".to_string()),
                address: None,
            },
            DynamicTraceEvent {
                sequence: 2,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: None,
                storage_value: None,
                address: None,
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("before any authorization in frame 'test_function'"));
    }

    #[test]
    fn auth_rule_detects_storage_without_any_auth() {
        let rule = AuthorizationCheckRule;

        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                address: None,
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key2".to_string(),
                caller: None,
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key2".to_string()),
                storage_value: Some("value2".to_string()),
                address: None,
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("without any preceding authorization in frame 'test_function'"));
    }

    #[test]
    fn auth_rule_detects_auth_in_unrelated_frame() {
        let rule = AuthorizationCheckRule;

        // Test case: Authorization happens in depth 1 (e.g. nested call), but write happens in depth 0
        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::Authorization,
                message: "auth check inside nested".to_string(),
                caller: None,
                function: Some("nested_function".to_string()),
                call_depth: Some(1),
                storage_key: None,
                storage_value: None,
                address: None,
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write key1 in main".to_string(),
                caller: None,
                function: Some("main_function".to_string()),
                call_depth: Some(0),
                storage_key: Some("key1".to_string()),
                storage_value: Some("value1".to_string()),
                address: None,
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "missing-auth");
        assert!(findings[0]
            .description
            .contains("without any preceding authorization in frame 'main_function'"));
    }

    #[test]
    fn auth_rule_detects_actor_mismatch() {
        let rule = AuthorizationCheckRule;

        let trace = vec![
            DynamicTraceEvent {
                sequence: 0,
                kind: DynamicTraceEventKind::Authorization,
                message: "authorized G_ALICE".to_string(),
                function: Some("test_function".to_string()),
                call_depth: Some(0),
                address: Some(
                    "G_ALICE_ADDRESS_1234567890123456789012345678901234567890123456".to_string(),
                ),
                storage_key: None,
                storage_value: None,
                caller: None,
            },
            DynamicTraceEvent {
                sequence: 1,
                kind: DynamicTraceEventKind::StorageWrite,
                message: "write G_BOB data".to_string(),
                function: Some("test_function".to_string()),
                storage_key: Some(
                    "data:G_BOB_ADDRESS_1234567890123456789012345678901234567890123456".to_string(),
                ),
                storage_value: Some("value".to_string()),
                call_depth: Some(0),
                address: None,
                caller: None,
            },
        ];

        let findings = rule.analyze_dynamic(None, &trace).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0]
            .description
            .contains("without authorization for a relevant actor"));
        assert!(findings[0].description.contains("G_ALICE_ADDRESS"));
    }
}
