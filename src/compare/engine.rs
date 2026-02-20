//! Comparison engine that loads two execution traces and produces a
//! structured report covering storage, budget, return values, and
//! execution flow differences.

use super::trace::{BudgetTrace, CallEntry, EventEntry, ExecutionTrace};
use std::collections::{BTreeMap, BTreeSet};

// ─── Diff types ──────────────────────────────────────────────────────

/// Overall comparison report returned by [`CompareEngine::compare`].
#[derive(Debug, Clone)]
pub struct ComparisonReport {
    pub label_a: String,
    pub label_b: String,
    pub storage_diff: StorageDiff,
    pub budget_diff: BudgetDiff,
    pub return_value_diff: ReturnValueDiff,
    pub flow_diff: FlowDiff,
    pub event_diff: EventDiff,
}

/// Storage key-level differences.
#[derive(Debug, Clone)]
pub struct StorageDiff {
    /// Keys present only in trace A
    pub only_in_a: BTreeMap<String, serde_json::Value>,
    /// Keys present only in trace B
    pub only_in_b: BTreeMap<String, serde_json::Value>,
    /// Keys present in both but with different values: key → (a_val, b_val)
    pub modified: BTreeMap<String, (serde_json::Value, serde_json::Value)>,
    /// Keys with identical values
    pub unchanged_count: usize,
}

/// Numeric deltas for resource budgets.
#[derive(Debug, Clone)]
pub struct BudgetDiff {
    pub a: Option<BudgetTrace>,
    pub b: Option<BudgetTrace>,
    /// Positive = B uses more; negative = B uses less
    pub cpu_delta: Option<i128>,
    pub memory_delta: Option<i128>,
}

/// Return value comparison.
#[derive(Debug, Clone)]
pub struct ReturnValueDiff {
    pub a: Option<serde_json::Value>,
    pub b: Option<serde_json::Value>,
    pub equal: bool,
}

/// Call-sequence comparison.
#[derive(Debug, Clone)]
pub struct FlowDiff {
    pub a_calls: Vec<CallEntry>,
    pub b_calls: Vec<CallEntry>,
    /// Unified diff lines (text representation)
    pub diff_lines: Vec<DiffLine>,
    pub identical: bool,
}

/// Event comparison.
#[derive(Debug, Clone)]
pub struct EventDiff {
    pub a_events: Vec<EventEntry>,
    pub b_events: Vec<EventEntry>,
    pub identical: bool,
}

/// A single line in a unified-style diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// Present in both traces at the same position.
    Same(String),
    /// Present only in trace A (removed in B).
    OnlyA(String),
    /// Present only in trace B (added in B).
    OnlyB(String),
}

// ─── Engine ──────────────────────────────────────────────────────────

/// The comparison engine.
pub struct CompareEngine;

impl CompareEngine {
    /// Compare two execution traces and produce a report.
    pub fn compare(trace_a: &ExecutionTrace, trace_b: &ExecutionTrace) -> ComparisonReport {
        let label_a = trace_a
            .label
            .clone()
            .unwrap_or_else(|| "Trace A".to_string());
        let label_b = trace_b
            .label
            .clone()
            .unwrap_or_else(|| "Trace B".to_string());

        ComparisonReport {
            label_a,
            label_b,
            storage_diff: Self::diff_storage(&trace_a.storage, &trace_b.storage),
            budget_diff: Self::diff_budget(&trace_a.budget, &trace_b.budget),
            return_value_diff: Self::diff_return_value(
                &trace_a.return_value,
                &trace_b.return_value,
            ),
            flow_diff: Self::diff_flow(&trace_a.call_sequence, &trace_b.call_sequence),
            event_diff: Self::diff_events(&trace_a.events, &trace_b.events),
        }
    }

    // ── Storage ──────────────────────────────────────────────────────

    fn diff_storage(
        a: &BTreeMap<String, serde_json::Value>,
        b: &BTreeMap<String, serde_json::Value>,
    ) -> StorageDiff {
        let keys_a: BTreeSet<_> = a.keys().cloned().collect();
        let keys_b: BTreeSet<_> = b.keys().cloned().collect();

        let mut only_in_a = BTreeMap::new();
        let mut only_in_b = BTreeMap::new();
        let mut modified = BTreeMap::new();
        let mut unchanged_count: usize = 0;

        for key in &keys_a {
            if !keys_b.contains(key) {
                only_in_a.insert(key.clone(), a[key].clone());
            }
        }

        for key in &keys_b {
            if !keys_a.contains(key) {
                only_in_b.insert(key.clone(), b[key].clone());
            }
        }

        for key in keys_a.intersection(&keys_b) {
            if a[key] != b[key] {
                modified.insert(key.clone(), (a[key].clone(), b[key].clone()));
            } else {
                unchanged_count += 1;
            }
        }

        StorageDiff {
            only_in_a,
            only_in_b,
            modified,
            unchanged_count,
        }
    }

    // ── Budget ───────────────────────────────────────────────────────

    fn diff_budget(a: &Option<BudgetTrace>, b: &Option<BudgetTrace>) -> BudgetDiff {
        let cpu_delta = match (a, b) {
            (Some(a), Some(b)) => Some(b.cpu_instructions as i128 - a.cpu_instructions as i128),
            _ => None,
        };
        let memory_delta = match (a, b) {
            (Some(a), Some(b)) => Some(b.memory_bytes as i128 - a.memory_bytes as i128),
            _ => None,
        };

        BudgetDiff {
            a: a.clone(),
            b: b.clone(),
            cpu_delta,
            memory_delta,
        }
    }

    // ── Return value ─────────────────────────────────────────────────

    fn diff_return_value(
        a: &Option<serde_json::Value>,
        b: &Option<serde_json::Value>,
    ) -> ReturnValueDiff {
        let equal = a == b;
        ReturnValueDiff {
            a: a.clone(),
            b: b.clone(),
            equal,
        }
    }

    // ── Execution flow (LCS-based unified diff) ──────────────────────

    fn diff_flow(a: &[CallEntry], b: &[CallEntry]) -> FlowDiff {
        let identical = a == b;
        let diff_lines = Self::compute_lcs_diff(a, b);

        FlowDiff {
            a_calls: a.to_vec(),
            b_calls: b.to_vec(),
            diff_lines,
            identical,
        }
    }

    /// Compute a unified-style diff of two call sequences using LCS.
    fn compute_lcs_diff(a: &[CallEntry], b: &[CallEntry]) -> Vec<DiffLine> {
        let n = a.len();
        let m = b.len();

        // Build LCS table
        let mut table = vec![vec![0u32; m + 1]; n + 1];
        for i in 1..=n {
            for j in 1..=m {
                if a[i - 1] == b[j - 1] {
                    table[i][j] = table[i - 1][j - 1] + 1;
                } else {
                    table[i][j] = table[i - 1][j].max(table[i][j - 1]);
                }
            }
        }

        // Back-track to produce diff
        let mut lines = Vec::new();
        let (mut i, mut j) = (n, m);

        while i > 0 || j > 0 {
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
                lines.push(DiffLine::Same(Self::format_call(&a[i - 1])));
                i -= 1;
                j -= 1;
            } else if j > 0 && (i == 0 || table[i][j - 1] >= table[i - 1][j]) {
                lines.push(DiffLine::OnlyB(Self::format_call(&b[j - 1])));
                j -= 1;
            } else {
                lines.push(DiffLine::OnlyA(Self::format_call(&a[i - 1])));
                i -= 1;
            }
        }

        lines.reverse();
        lines
    }

    fn format_call(entry: &CallEntry) -> String {
        let indent = "  ".repeat(entry.depth as usize);
        if let Some(ref args) = entry.args {
            format!("{}{}({})", indent, entry.function, args)
        } else {
            format!("{}{}()", indent, entry.function)
        }
    }

    // ── Events ───────────────────────────────────────────────────────

    fn diff_events(a: &[EventEntry], b: &[EventEntry]) -> EventDiff {
        let identical = a == b;
        EventDiff {
            a_events: a.to_vec(),
            b_events: b.to_vec(),
            identical,
        }
    }

    // ── Report rendering ─────────────────────────────────────────────

    /// Render the comparison report as a human-readable string.
    pub fn render_report(report: &ComparisonReport) -> String {
        let mut out = String::new();

        out.push_str("═══════════════════════════════════════════════════════════════\n");
        out.push_str("  Execution Trace Comparison\n");
        out.push_str(&format!(
            "  A: {}\n  B: {}\n",
            report.label_a, report.label_b
        ));
        out.push_str("═══════════════════════════════════════════════════════════════\n\n");

        // ── Storage ────────────────────────────────────────────────
        out.push_str("───────────────── Storage Changes ─────────────────\n\n");
        let sd = &report.storage_diff;

        if sd.only_in_a.is_empty() && sd.only_in_b.is_empty() && sd.modified.is_empty() {
            out.push_str("  (identical)\n");
        } else {
            if !sd.only_in_a.is_empty() {
                out.push_str(&format!("  Keys only in A ({}):\n", sd.only_in_a.len()));
                for (k, v) in &sd.only_in_a {
                    out.push_str(&format!("    - {} = {}\n", k, v));
                }
                out.push('\n');
            }

            if !sd.only_in_b.is_empty() {
                out.push_str(&format!("  Keys only in B ({}):\n", sd.only_in_b.len()));
                for (k, v) in &sd.only_in_b {
                    out.push_str(&format!("    + {} = {}\n", k, v));
                }
                out.push('\n');
            }

            if !sd.modified.is_empty() {
                out.push_str(&format!("  Modified keys ({}):\n", sd.modified.len()));
                for (k, (va, vb)) in &sd.modified {
                    out.push_str(&format!("    ~ {}\n", k));
                    out.push_str(&format!("        A: {}\n", va));
                    out.push_str(&format!("        B: {}\n", vb));
                }
                out.push('\n');
            }

            out.push_str(&format!("  Unchanged keys: {}\n", sd.unchanged_count));
        }
        out.push('\n');

        // ── Budget ─────────────────────────────────────────────────
        out.push_str("───────────────── Budget Usage ────────────────────\n\n");
        let bd = &report.budget_diff;

        match (&bd.a, &bd.b) {
            (Some(a), Some(b)) => {
                out.push_str(&format!(
                    "  {:>28}  {:>14}  {:>14}  {:>14}\n",
                    "", "A", "B", "Delta"
                ));
                out.push_str(&format!(
                    "  {:>28}  {:>14}  {:>14}  {:>+14}\n",
                    "CPU instructions",
                    a.cpu_instructions,
                    b.cpu_instructions,
                    bd.cpu_delta.unwrap_or(0)
                ));
                out.push_str(&format!(
                    "  {:>28}  {:>14}  {:>14}  {:>+14}\n",
                    "Memory (bytes)",
                    a.memory_bytes,
                    b.memory_bytes,
                    bd.memory_delta.unwrap_or(0)
                ));

                // Percentage change
                if a.cpu_instructions > 0 {
                    let pct =
                        (bd.cpu_delta.unwrap_or(0) as f64 / a.cpu_instructions as f64) * 100.0;
                    out.push_str(&format!("\n  CPU change: {:+.2}%\n", pct));
                }
                if a.memory_bytes > 0 {
                    let pct = (bd.memory_delta.unwrap_or(0) as f64 / a.memory_bytes as f64) * 100.0;
                    out.push_str(&format!("  Memory change: {:+.2}%\n", pct));
                }
            }
            (None, None) => {
                out.push_str("  (no budget data in either trace)\n");
            }
            (Some(a), None) => {
                out.push_str(&format!(
                    "  A: CPU={}, Mem={}\n  B: (no budget data)\n",
                    a.cpu_instructions, a.memory_bytes
                ));
            }
            (None, Some(b)) => {
                out.push_str(&format!(
                    "  A: (no budget data)\n  B: CPU={}, Mem={}\n",
                    b.cpu_instructions, b.memory_bytes
                ));
            }
        }
        out.push('\n');

        // ── Return values ──────────────────────────────────────────
        out.push_str("───────────────── Return Values ───────────────────\n\n");
        let rv = &report.return_value_diff;

        if rv.equal {
            match &rv.a {
                Some(v) => out.push_str(&format!("  (identical) {}\n", v)),
                None => out.push_str("  (both traces have no return value)\n"),
            }
        } else {
            out.push_str(&format!(
                "  A: {}\n  B: {}\n",
                rv.a.as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "(none)".to_string()),
                rv.b.as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "(none)".to_string()),
            ));
        }
        out.push('\n');

        // ── Execution flow ─────────────────────────────────────────
        out.push_str("───────────────── Execution Flow ──────────────────\n\n");
        let fd = &report.flow_diff;

        if fd.identical {
            out.push_str("  (identical call sequences)\n");
            for entry in &fd.a_calls {
                out.push_str(&format!("    {}\n", Self::format_call_display(entry)));
            }
        } else {
            out.push_str("  Unified diff (- = only in A, + = only in B):\n\n");
            for line in &fd.diff_lines {
                match line {
                    DiffLine::Same(s) => out.push_str(&format!("    {}\n", s)),
                    DiffLine::OnlyA(s) => out.push_str(&format!("  - {}\n", s)),
                    DiffLine::OnlyB(s) => out.push_str(&format!("  + {}\n", s)),
                }
            }
        }
        out.push('\n');

        // ── Events ─────────────────────────────────────────────────
        out.push_str("───────────────── Events ──────────────────────────\n\n");
        let ed = &report.event_diff;

        if ed.identical {
            if ed.a_events.is_empty() {
                out.push_str("  (no events in either trace)\n");
            } else {
                out.push_str(&format!("  (identical — {} event(s))\n", ed.a_events.len()));
            }
        } else {
            out.push_str(&format!(
                "  A: {} event(s), B: {} event(s)\n\n",
                ed.a_events.len(),
                ed.b_events.len()
            ));

            out.push_str("  Events in A:\n");
            for (i, ev) in ed.a_events.iter().enumerate() {
                out.push_str(&format!("    [{}] topics={:?}", i, ev.topics));
                if let Some(ref d) = ev.data {
                    out.push_str(&format!(" data={}", d));
                }
                out.push('\n');
            }

            out.push_str("\n  Events in B:\n");
            for (i, ev) in ed.b_events.iter().enumerate() {
                out.push_str(&format!("    [{}] topics={:?}", i, ev.topics));
                if let Some(ref d) = ev.data {
                    out.push_str(&format!(" data={}", d));
                }
                out.push('\n');
            }
        }

        out.push_str("\n═══════════════════════════════════════════════════════════════\n");

        out
    }

    fn format_call_display(entry: &CallEntry) -> String {
        let indent = "  ".repeat(entry.depth as usize);
        if let Some(ref args) = entry.args {
            format!("{}{}({})", indent, entry.function, args)
        } else {
            format!("{}{}()", indent, entry.function)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::trace::*;

    fn make_trace_a() -> ExecutionTrace {
        ExecutionTrace {
            label: Some("v1.0 baseline".to_string()),
            contract: Some("token.wasm".to_string()),
            function: Some("transfer".to_string()),
            args: Some(r#"["Alice","Bob",100]"#.to_string()),
            storage: BTreeMap::from([
                ("balance:Alice".to_string(), serde_json::json!(900)),
                ("balance:Bob".to_string(), serde_json::json!(100)),
                ("total_supply".to_string(), serde_json::json!(1000)),
            ]),
            budget: Some(BudgetTrace {
                cpu_instructions: 45000,
                memory_bytes: 15360,
                cpu_limit: Some(100000),
                memory_limit: Some(40960),
            }),
            return_value: Some(serde_json::json!({"status": "ok"})),
            call_sequence: vec![
                CallEntry {
                    function: "transfer".to_string(),
                    args: None,
                    depth: 0,
                },
                CallEntry {
                    function: "get_balance".to_string(),
                    args: Some("Alice".to_string()),
                    depth: 1,
                },
                CallEntry {
                    function: "set_balance".to_string(),
                    args: Some("Alice, 900".to_string()),
                    depth: 1,
                },
                CallEntry {
                    function: "set_balance".to_string(),
                    args: Some("Bob, 100".to_string()),
                    depth: 1,
                },
            ],
            events: vec![EventEntry {
                contract_id: Some("TOKEN01".to_string()),
                topics: vec!["transfer".to_string()],
                data: Some("Alice→Bob 100".to_string()),
            }],
        }
    }

    fn make_trace_b() -> ExecutionTrace {
        ExecutionTrace {
            label: Some("v1.1 optimized".to_string()),
            contract: Some("token.wasm".to_string()),
            function: Some("transfer".to_string()),
            args: Some(r#"["Alice","Bob",100]"#.to_string()),
            storage: BTreeMap::from([
                ("balance:Alice".to_string(), serde_json::json!(900)),
                ("balance:Bob".to_string(), serde_json::json!(150)),
                ("total_supply".to_string(), serde_json::json!(1050)),
                ("fee_pool".to_string(), serde_json::json!(50)),
            ]),
            budget: Some(BudgetTrace {
                cpu_instructions: 38000,
                memory_bytes: 14000,
                cpu_limit: Some(100000),
                memory_limit: Some(40960),
            }),
            return_value: Some(serde_json::json!({"status": "ok", "fee": 0})),
            call_sequence: vec![
                CallEntry {
                    function: "transfer".to_string(),
                    args: None,
                    depth: 0,
                },
                CallEntry {
                    function: "check_allowance".to_string(),
                    args: Some("Alice".to_string()),
                    depth: 1,
                },
                CallEntry {
                    function: "get_balance".to_string(),
                    args: Some("Alice".to_string()),
                    depth: 1,
                },
                CallEntry {
                    function: "set_balance".to_string(),
                    args: Some("Alice, 900".to_string()),
                    depth: 1,
                },
                CallEntry {
                    function: "set_balance".to_string(),
                    args: Some("Bob, 150".to_string()),
                    depth: 1,
                },
            ],
            events: vec![
                EventEntry {
                    contract_id: Some("TOKEN01".to_string()),
                    topics: vec!["transfer".to_string()],
                    data: Some("Alice→Bob 100".to_string()),
                },
                EventEntry {
                    contract_id: Some("TOKEN01".to_string()),
                    topics: vec!["fee".to_string()],
                    data: Some("50".to_string()),
                },
            ],
        }
    }

    #[test]
    fn test_storage_diff_detects_changes() {
        let a = make_trace_a();
        let b = make_trace_b();
        let report = CompareEngine::compare(&a, &b);

        // balance:Bob changed 100 → 150
        assert!(report.storage_diff.modified.contains_key("balance:Bob"));
        // total_supply changed
        assert!(report.storage_diff.modified.contains_key("total_supply"));
        // fee_pool only in B
        assert!(report.storage_diff.only_in_b.contains_key("fee_pool"));
        // balance:Alice unchanged
        assert_eq!(report.storage_diff.unchanged_count, 1);
        // nothing only in A
        assert!(report.storage_diff.only_in_a.is_empty());
    }

    #[test]
    fn test_budget_diff_computes_deltas() {
        let a = make_trace_a();
        let b = make_trace_b();
        let report = CompareEngine::compare(&a, &b);

        // B used fewer CPU instructions
        assert_eq!(report.budget_diff.cpu_delta, Some(-7000));
        // B used less memory
        assert_eq!(report.budget_diff.memory_delta, Some(-1360));
    }

    #[test]
    fn test_return_value_diff_not_equal() {
        let a = make_trace_a();
        let b = make_trace_b();
        let report = CompareEngine::compare(&a, &b);

        assert!(!report.return_value_diff.equal);
    }

    #[test]
    fn test_return_value_diff_equal() {
        let a = make_trace_a();
        let mut b = make_trace_b();
        b.return_value = a.return_value.clone();
        let report = CompareEngine::compare(&a, &b);

        assert!(report.return_value_diff.equal);
    }

    #[test]
    fn test_flow_diff_detects_difference() {
        let a = make_trace_a();
        let b = make_trace_b();
        let report = CompareEngine::compare(&a, &b);

        assert!(!report.flow_diff.identical);
        // The diff should contain at least one OnlyB line (check_allowance)
        assert!(report
            .flow_diff
            .diff_lines
            .iter()
            .any(|l| matches!(l, DiffLine::OnlyB(_))));
    }

    #[test]
    fn test_flow_diff_identical() {
        let a = make_trace_a();
        let mut b = make_trace_a();
        b.label = Some("copy".to_string());
        let report = CompareEngine::compare(&a, &b);

        assert!(report.flow_diff.identical);
    }

    #[test]
    fn test_event_diff_detects_difference() {
        let a = make_trace_a();
        let b = make_trace_b();
        let report = CompareEngine::compare(&a, &b);

        assert!(!report.event_diff.identical);
    }

    #[test]
    fn test_render_report_no_panic() {
        let a = make_trace_a();
        let b = make_trace_b();
        let report = CompareEngine::compare(&a, &b);
        let rendered = CompareEngine::render_report(&report);

        assert!(rendered.contains("Storage Changes"));
        assert!(rendered.contains("Budget Usage"));
        assert!(rendered.contains("Return Values"));
        assert!(rendered.contains("Execution Flow"));
        assert!(rendered.contains("Events"));
    }

    #[test]
    fn test_identical_traces() {
        let a = make_trace_a();
        let b = make_trace_a();
        let report = CompareEngine::compare(&a, &b);

        assert!(report.storage_diff.only_in_a.is_empty());
        assert!(report.storage_diff.only_in_b.is_empty());
        assert!(report.storage_diff.modified.is_empty());
        assert!(report.return_value_diff.equal);
        assert!(report.flow_diff.identical);
        assert!(report.event_diff.identical);
        assert_eq!(report.budget_diff.cpu_delta, Some(0));
        assert_eq!(report.budget_diff.memory_delta, Some(0));
    }

    #[test]
    fn test_missing_budget_in_one_trace() {
        let a = make_trace_a();
        let mut b = make_trace_b();
        b.budget = None;
        let report = CompareEngine::compare(&a, &b);

        assert!(report.budget_diff.cpu_delta.is_none());
        assert!(report.budget_diff.memory_delta.is_none());
    }
}
