//! Output and accessibility configuration for screen-reader compatible CLI.
//!
//! Supports `NO_COLOR` (disable ANSI colors) and `--no-unicode` (ASCII-only output).

use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::inspector::budget::{BudgetInspector, ResourceCheckpoint};

static NO_UNICODE: AtomicBool = AtomicBool::new(false);
static COLORS_ENABLED: AtomicBool = AtomicBool::new(true);
pub const SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Notice,
    Warning,
    Error,
}

impl DiagnosticSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Notice => "NOTICE",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub source: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub severity: DiagnosticSeverity,
}

impl DiagnosticRecord {
    pub fn new(
        source: impl Into<String>,
        summary: impl Into<String>,
        detail: Option<String>,
        severity: DiagnosticSeverity,
    ) -> Self {
        Self {
            source: source.into(),
            summary: summary.into(),
            detail,
            severity,
        }
    }

    pub fn display_line(&self) -> String {
        match &self.detail {
            Some(detail) if !detail.is_empty() => format!(
                "[{}] {}: {} ({})",
                self.severity.label(),
                self.source,
                self.summary,
                detail
            ),
            _ => format!(
                "[{}] {}: {}",
                self.severity.label(),
                self.source,
                self.summary
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputError {
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginIncidentType {
    Panic,
    Timeout,
}

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InvocationReason {
    Entrypoint,
    CrossContract,
    Replay,
    Plugin,
}

impl InvocationReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entrypoint => "entrypoint",
            Self::CrossContract => "cross_contract",
            Self::Replay => "replay",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayArtifactKind {
    Manifest,
    Trace,
    ContractWasm,
    NetworkSnapshot,
    StorageImport,
    StorageExport,
    OutputReport,
    GeneratedTest,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ReplayArtifactFile {
    pub kind: ReplayArtifactKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compression: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ReplayArtifactManifest {
    pub schema_version: String,
    pub artifact_group: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    pub files: Vec<ReplayArtifactFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginIncidentReport {
    pub plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_path: Option<String>,
    pub invocation_kind: String,
    pub incident: PluginIncidentType,
    pub action_taken: String,
    pub core_debugger_status: String,
    pub message: String,
}

impl PluginIncidentReport {
    pub fn summary_line(&self) -> String {
        format!(
            "Plugin incident: '{}' {} during {}. Action: {}. Core debugger status: {}.",
            self.plugin,
            match self.incident {
                PluginIncidentType::Panic => "panicked",
                PluginIncidentType::Timeout => "timed out",
            },
            self.invocation_kind,
            self.action_taken,
            self.core_debugger_status
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginReloadOutcome {
    Success,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct PluginReloadReport {
    pub plugin: String,
    pub timestamp: String,
    pub outcome: PluginReloadOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserved_state_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl PluginReloadReport {
    pub fn summary_line(&self) -> String {
        match self.outcome {
            PluginReloadOutcome::Success => {
                let size = self.preserved_state_bytes.map_or("unknown".to_string(), |b| format!("{} bytes", b));
                format!("Plugin '{}' reloaded successfully. Preserved state: {}.", self.plugin, size)
            }
            PluginReloadOutcome::Failed => {
                let reason = self.reason.as_deref().unwrap_or("unknown error");
                format!("Plugin '{}' reload failed: {}.", self.plugin, reason)
            }
            PluginReloadOutcome::RolledBack => {
                let reason = self.reason.as_deref().unwrap_or("unknown error");
                format!("Plugin '{}' reload rolled back. Reason: {}.", self.plugin, reason)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BatchResultKind {
    Passed,
    Failed,
    Panicked,
    TimedOut,
    Skipped,
    Error,
}

impl BatchResultKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Passed => "Passed",
            Self::Failed => "Failed",
            Self::Panicked => "Panicked",
            Self::TimedOut => "Timed Out",
            Self::Skipped => "Skipped",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub label: String,
    pub kind: BatchResultKind,
    pub args: String,
    pub result: Option<String>,
    pub expected: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BatchSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub panicked: usize,
    pub timed_out: usize,
    pub skipped: usize,
    pub errors: usize,
    pub total_duration_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecutionResult {
    pub summary: BatchSummary,
    pub results: Vec<BatchResult>,
}

pub fn collect_runtime_diagnostics(
    source_map_loaded: bool,
    budget: &crate::inspector::budget::BudgetInfo,
    last_error: Option<&str>,
) -> Vec<DiagnosticRecord> {
    let mut diagnostics = Vec::new();

    if !source_map_loaded {
        diagnostics.push(DiagnosticRecord::new(
            "source_map",
            "Source locations are degraded for this session.",
            Some(
                "DWARF/source map data could not be loaded, so paused file and line hints may be unavailable."
                    .to_string(),
            ),
            DiagnosticSeverity::Warning,
        ));
    }

    for (resource, percentage) in [
        ("CPU", budget.cpu_percentage()),
        ("Memory", budget.memory_percentage()),
    ] {
        let severity = if percentage >= 90.0 {
            Some(DiagnosticSeverity::Warning)
        } else if percentage >= 70.0 {
            Some(DiagnosticSeverity::Notice)
        } else {
            None
        };

        if let Some(severity) = severity {
            let detail = if percentage >= 90.0 {
                Some(format!(
                    "{} usage is at {:.1}% of the configured limit. Consider reducing contract work or data size.",
                    resource, percentage
                ))
            } else {
                Some(format!(
                    "{} usage is at {:.1}% of the configured limit.",
                    resource, percentage
                ))
            };
            diagnostics.push(DiagnosticRecord::new(
                format!("budget/{}", resource.to_lowercase()),
                format!("{} budget is running high.", resource),
                detail,
                severity,
            ));
        }
    }

    if let Some(error) = last_error.filter(|error| !error.trim().is_empty()) {
        diagnostics.push(DiagnosticRecord::new(
            "execution",
            "The most recent debugger action failed.",
            Some(error.to_string()),
            DiagnosticSeverity::Error,
        ));
    }

    diagnostics
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionedOutput<T>
where
    T: Serialize,
{
    pub schema_version: &'static str,
    pub command: String,
    pub status: OutputStatus,
    pub result: Option<T>,
    pub error: Option<OutputError>,
}

impl<T> VersionedOutput<T>
where
    T: Serialize,
{
    pub fn success(command: impl Into<String>, result: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            command: command.into(),
            status: OutputStatus::Success,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(command: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            command: command.into(),
            status: OutputStatus::Error,
            result: None,
            error: Some(OutputError {
                message: message.into(),
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SymbolicReplayBundle {
    pub schema_version: u8,
    pub command: String,

    pub contract: ContractInfo,
    pub invocation: InvocationInfo,
    pub config: ReplayConfig,

    pub storage_seed: Option<StorageSeed>,
    pub metadata: Option<ReplayMetadata>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContractInfo {
    pub sha256: String,
    pub path_hint: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InvocationInfo {
    pub function: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReplayConfig {
    pub seed: Option<u64>,
    pub max_paths: Option<usize>,
    pub max_input_combinations: Option<usize>,
    pub max_breadth: Option<usize>,
    pub max_depth: Option<usize>,
    pub timeout_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StorageSeed {
    pub format: String,
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReplayMetadata {
    pub paths_explored: usize,
    pub panics_found: usize,
}

/// Global output/accessibility configuration.
pub struct OutputConfig;

impl OutputConfig {
    /// Configure from CLI flags and environment.
    /// Call once at startup after parsing args.
    pub fn configure(no_unicode: bool) {
        NO_UNICODE.store(no_unicode, Ordering::Relaxed);
        // NO_COLOR: if set and not empty, disable ANSI colors
        let no_color = std::env::var("NO_COLOR")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        COLORS_ENABLED.store(!no_color, Ordering::Relaxed);
    }

    /// Whether `--no-unicode` is active (use ASCII-only output).
    #[inline]
    pub fn no_unicode() -> bool {
        NO_UNICODE.load(Ordering::Relaxed)
    }

    /// Whether ANSI colors are enabled (false when NO_COLOR is set and not empty).
    #[inline]
    pub fn colors_enabled() -> bool {
        COLORS_ENABLED.load(Ordering::Relaxed)
    }

    /// Replace box-drawing and other Unicode symbols with ASCII when `--no-unicode` is set.
    pub fn to_ascii(s: &str) -> String {
        if !Self::no_unicode() {
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            out.push(Self::replace_unicode_char(c));
        }
        out
    }

    fn replace_unicode_char(c: char) -> char {
        match c {
            // Corners
            '┌' | '┐' | '└' | '┘' => '+',
            // Horizontal
            '─' | '━' | '═' => '-',
            // Vertical
            '│' | '┃' => '|',
            // T-junctions
            '┬' | '┴' | '├' | '┤' | '┼' => '+',
            // Bullets / markers
            '•' => '*',
            '→' => '>',
            '⚠' => '!',
            '✔' | '✓' => '+',
            '✗' | '✘' => 'x',
            _ => c,
        }
    }

    /// Horizontal rule character(s) for section separators.
    pub fn rule_char() -> &'static str {
        "-"
    }

    /// Double-line rule character for headers.
    pub fn double_rule_char() -> &'static str {
        if Self::no_unicode() {
            "="
        } else {
            "\u{2550}" // ═
        }
    }

    /// A horizontal rule line (single line, for section separators).
    pub fn rule_line(len: usize) -> String {
        Self::rule_char().repeat(len)
    }

    /// A double horizontal rule line (for headers).
    pub fn double_rule_line(len: usize) -> String {
        Self::double_rule_char().repeat(len)
    }
}


/// Status kind for text-equivalent labels (screen reader friendly).
#[derive(Clone, Copy)]
pub enum StatusLabel {
    Pass,
    Fail,
    Info,
    Warning,
    Error,
    Working,
}

impl StatusLabel {
    /// Text label to use when color is disabled or for accessibility.
    pub fn as_str(self) -> &'static str {
        match self {
            StatusLabel::Pass => "[PASS]",
            StatusLabel::Fail => "[FAIL]",
            StatusLabel::Info => "[INFO]",
            StatusLabel::Warning => "[WARN]",
            StatusLabel::Error => "[ERROR]",
            StatusLabel::Working => "[WORKING...]",
        }
    }
}

/// Spinner / progress: in no-unicode or accessibility mode, return static text instead of Unicode spinner.
pub fn spinner_text() -> &'static str {
    "[WORKING...]"
}

/// Helper for writing output to both stdout and optionally to a file
pub struct OutputWriter {
    file: Option<std::fs::File>,
}

impl OutputWriter {
    /// Create a new OutputWriter that optionally writes to a file
    pub fn new(path: Option<&std::path::Path>, append: bool) -> miette::Result<Self> {
        let file = if let Some(p) = path {
            if append {
                Some(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(p)
                        .map_err(|e| miette::miette!("Failed to open output file: {}", e))?,
                )
            } else {
                Some(
                    std::fs::File::create(p)
                        .map_err(|e| miette::miette!("Failed to create output file: {}", e))?,
                )
            }
        } else {
            None
        };
        Ok(Self { file })
    }

    /// Write a line to the file (if configured)
    pub fn write(&mut self, text: &str) -> miette::Result<()> {
        if let Some(ref mut f) = self.file {
            use std::io::Write;
            writeln!(f, "{}", text)
                .map_err(|e| miette::miette!("Failed to write to output file: {}", e))?;
        }
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replay_bundle_serializes() {
        let bundle = SymbolicReplayBundle {
            schema_version: 1,
            command: "symbolic".to_string(),
            contract: ContractInfo {
                sha256: "abc".to_string(),
                path_hint: None,
            },
            invocation: InvocationInfo {
                function: "test".to_string(),
            },
            config: ReplayConfig {
                seed: Some(1),
                max_paths: None,
                max_input_combinations: None,
                max_breadth: None,
                max_depth: None,
                timeout_secs: None,
            },
            storage_seed: None,
            metadata: None,
        };

        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("schema_version"));
        assert!(json.contains("symbolic"));
    }

    #[test]
    fn test_replay_bundle_optional_fields_serialization() {
        let bundle = SymbolicReplayBundle {
            schema_version: 1,
            command: "symbolic".to_string(),
            contract: ContractInfo {
                sha256: "abc".to_string(),
                path_hint: Some("contract.wasm".to_string()),
            },
            invocation: InvocationInfo {
                function: "test".to_string(),
            },
            config: ReplayConfig {
                seed: None,
                max_paths: Some(10),
                max_input_combinations: Some(20),
                max_breadth: Some(2),
                max_depth: Some(3),
                timeout_secs: Some(30),
            },
            storage_seed: Some(StorageSeed {
                format: "json".to_string(),
                data: "{}".to_string(),
            }),
            metadata: Some(ReplayMetadata {
                paths_explored: 5,
                panics_found: 1,
            }),
        };

        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("storage_seed"));
        assert!(json.contains("metadata"));
        assert!(json.contains("contract.wasm"));
    }
}

/// Formats a resource usage timeline as a human-readable table.
pub fn format_resource_timeline(
    timeline: &[crate::inspector::budget::ResourceCheckpoint],
) -> String {
    let mut output = String::new();
    let mut last_cpu = 0;
    let mut last_mem = 0;

    output.push_str("| Time | Location | Total CPU | CPU Delta | Total Mem | Mem Delta |\n");
    output.push_str("|------|----------|-----------|-----------|-----------|-----------|\n");

    for checkpoint in timeline {
        let cpu_delta = checkpoint.cpu_instructions.saturating_sub(last_cpu);
        let mem_delta = checkpoint.memory_bytes.saturating_sub(last_mem);

        let _ = writeln!(
            output,
            "| {: >4}ms | {: <8} | {: >9} | {: >9} | {: >9} | {: >9} |",
            checkpoint.timestamp_ms,
            if checkpoint.location_name.len() > 8 {
                &checkpoint.location_name[..8]
            } else {
                &checkpoint.location_name
            },
            checkpoint.cpu_instructions,
            cpu_delta,
            checkpoint.memory_bytes,
            mem_delta
        );

        last_cpu = checkpoint.cpu_instructions;
        last_mem = checkpoint.memory_bytes;
    }
    output
}

/// Formats a compact "spike triage" view over a resource checkpoint timeline.
///
/// Intended for normal debugger output: quickly identify where budget spikes happened
/// without switching to the full profiler.
pub fn format_budget_delta_summary(timeline: &[ResourceCheckpoint], top_n: usize) -> String {
    let deltas = BudgetInspector::deltas_from_checkpoints(timeline);
    if deltas.is_empty() {
        return String::new();
    }

    let spikes = BudgetInspector::top_spikes(&deltas, top_n.max(1));

    fn format_row(d: &crate::inspector::budget::BudgetDeltaCheckpoint) -> String {
        let loc = if d.location_name.len() > 36 {
            format!("{}…", &d.location_name[..35])
        } else {
            d.location_name.clone()
        };

        format!(
            "  {t:>4}ms  cpu +{cpu:<10} mem +{mem:<10}  {loc}",
            t = d.timestamp_ms,
            cpu = crate::ui::formatter::Formatter::format_compact_u64(d.cpu_delta),
            mem = crate::ui::formatter::Formatter::format_bytes(d.mem_delta),
            loc = loc
        )
    }

    let mut out = String::new();
    let _ = writeln!(&mut out, "=== Budget deltas (top spikes) ===");
    let _ = writeln!(
        &mut out,
        "Captured {} checkpoints ({} deltas).",
        timeline.len(),
        deltas.len()
    );
    let _ = writeln!(&mut out);

    let _ = writeln!(&mut out, "-- CPU spikes (top {}) --", spikes.by_cpu.len());
    for d in &spikes.by_cpu {
        let _ = writeln!(&mut out, "{}", format_row(d));
    }

    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "-- Memory spikes (top {}) --", spikes.by_mem.len());
    for d in &spikes.by_mem {
        let _ = writeln!(&mut out, "{}", format_row(d));
    }

    let _ = writeln!(&mut out);
    let _ = writeln!(
        &mut out,
        "-- Combined spikes (cpu_delta + mem_delta, top {}) --",
        spikes.by_combined.len()
    );
    for d in &spikes.by_combined {
        let _ = writeln!(&mut out, "{}", format_row(d));
    }

    out
}
