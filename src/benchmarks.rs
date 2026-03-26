use crate::{DebuggerError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CriterionBaseline {
    /// Map of benchmark ID -> mean point estimate (nanoseconds)
    pub mean_ns: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegressionStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Copy)]
pub struct ComparisonConfig {
    pub warn_pct: f64,
    pub fail_pct: f64,
}

impl Default for ComparisonConfig {
    fn default() -> Self {
        Self {
            warn_pct: 10.0,
            fail_pct: 20.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BenchmarkDelta {
    pub id: String,
    pub baseline_ns: f64,
    pub current_ns: f64,
    pub delta_pct: f64,
    pub status: RegressionStatus,
}

pub fn load_baseline_json(path: impl AsRef<Path>) -> Result<CriterionBaseline> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|e| {
        DebuggerError::FileError(format!("Failed to read baseline JSON {:?}: {e}", path))
    })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        DebuggerError::FileError(format!("Failed to parse baseline JSON {:?}: {e}", path)).into()
    })
}

pub fn write_baseline_json(path: impl AsRef<Path>, baseline: &CriterionBaseline) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                DebuggerError::FileError(format!("Failed to create directory {:?}: {e}", parent))
            })?;
        }
    }

    let json = serde_json::to_string_pretty(baseline).map_err(|e| {
        DebuggerError::FileError(format!("Failed to serialize baseline JSON {:?}: {e}", path))
    })?;
    fs::write(path, json).map_err(|e| {
        DebuggerError::FileError(format!("Failed to write baseline JSON {:?}: {e}", path))
    })?;
    Ok(())
}

pub fn collect_criterion_baseline(criterion_dir: impl AsRef<Path>) -> Result<CriterionBaseline> {
    let criterion_dir = criterion_dir.as_ref();
    let mut baseline = CriterionBaseline::default();

    let mut estimates = Vec::new();
    collect_estimates_files(criterion_dir, &mut estimates)?;

    for path in estimates {
        if let Some((id, mean)) = parse_estimates_mean_ns(criterion_dir, &path)? {
            baseline.mean_ns.insert(id, mean);
        }
    }

    Ok(baseline)
}

fn collect_estimates_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let dir = match fs::read_dir(root) {
        Ok(dir) => dir,
        Err(e) => {
            return Err(DebuggerError::FileError(format!(
                "Failed to read directory {:?}: {e}",
                root
            ))
            .into())
        }
    };

    for entry in dir {
        let entry = entry.map_err(|e| {
            DebuggerError::FileError(format!("Failed to read directory entry in {:?}: {e}", root))
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| DebuggerError::FileError(format!("Failed to stat {:?}: {e}", path)))?;

        if file_type.is_dir() {
            collect_estimates_files(&path, out)?;
            continue;
        }

        if file_type.is_file()
            && path.file_name().and_then(|s| s.to_str()) == Some("estimates.json")
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                == Some("new")
        {
            out.push(path);
        }
    }

    Ok(())
}

fn parse_estimates_mean_ns(
    criterion_dir: &Path,
    estimates_path: &Path,
) -> Result<Option<(String, f64)>> {
    let bytes = fs::read(estimates_path).map_err(|e| {
        DebuggerError::FileError(format!(
            "Failed to read estimates file {:?}: {e}",
            estimates_path
        ))
    })?;

    let json: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
        DebuggerError::FileError(format!(
            "Failed to parse estimates JSON {:?}: {e}",
            estimates_path
        ))
    })?;

    let mean = json
        .get("mean")
        .and_then(|m| m.get("point_estimate"))
        .and_then(|v| v.as_f64());

    let Some(mean_ns) = mean else {
        return Ok(None);
    };

    // Build a stable benchmark ID from the relative path:
    // <criterion_dir>/<id>/new/estimates.json
    let rel = estimates_path
        .strip_prefix(criterion_dir)
        .unwrap_or(estimates_path);
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    // Drop trailing "new/estimates.json"
    if parts.len() >= 2 {
        parts.truncate(parts.len().saturating_sub(2));
    }

    if parts.is_empty() {
        return Ok(None);
    }

    let id = parts.join("/");
    Ok(Some((id, mean_ns)))
}

pub fn compare_baselines(
    baseline: &CriterionBaseline,
    current: &CriterionBaseline,
    config: ComparisonConfig,
) -> Vec<BenchmarkDelta> {
    let warn = config.warn_pct.max(0.0);
    let fail = config.fail_pct.max(warn);

    let mut deltas: Vec<BenchmarkDelta> = Vec::new();

    for (id, baseline_ns) in &baseline.mean_ns {
        let Some(current_ns) = current.mean_ns.get(id) else {
            continue;
        };
        if *baseline_ns <= 0.0 || *current_ns <= 0.0 {
            continue;
        }

        let delta_pct = ((*current_ns - *baseline_ns) / *baseline_ns) * 100.0;

        let status = if delta_pct >= fail {
            RegressionStatus::Fail
        } else if delta_pct >= warn {
            RegressionStatus::Warn
        } else {
            RegressionStatus::Pass
        };

        deltas.push(BenchmarkDelta {
            id: id.clone(),
            baseline_ns: *baseline_ns,
            current_ns: *current_ns,
            delta_pct,
            status,
        });
    }

    // Largest regressions first, then improvements.
    deltas.sort_by(|a, b| {
        b.delta_pct
            .partial_cmp(&a.delta_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deltas
}

pub fn overall_status(deltas: &[BenchmarkDelta]) -> RegressionStatus {
    if deltas.iter().any(|d| d.status == RegressionStatus::Fail) {
        return RegressionStatus::Fail;
    }
    if deltas.iter().any(|d| d.status == RegressionStatus::Warn) {
        return RegressionStatus::Warn;
    }
    RegressionStatus::Pass
}

pub fn render_markdown_report(
    deltas: &[BenchmarkDelta],
    config: ComparisonConfig,
    max_rows: usize,
) -> String {
    let mut out = String::new();
    out.push_str("# Benchmark regression report\n\n");
    out.push_str(&format!(
        "- Policy: warn ≥ {:.1}%  fail ≥ {:.1}%\n",
        config.warn_pct, config.fail_pct
    ));

    let status = overall_status(deltas);
    out.push_str(&format!("- Status: {:?}\n\n", status));

    if deltas.is_empty() {
        out.push_str("No overlapping benchmarks found between baseline and current results.\n");
        return out;
    }

    out.push_str("| Benchmark | Baseline (ns) | Current (ns) | Δ% | Status |\n");
    out.push_str("|---|---:|---:|---:|---|\n");

    for d in deltas.iter().take(max_rows.max(1)) {
        out.push_str(&format!(
            "| `{}` | {:.1} | {:.1} | {:+.2}% | {:?} |\n",
            d.id, d.baseline_ns, d.current_ns, d.delta_pct, d.status
        ));
    }

    if deltas.len() > max_rows.max(1) {
        out.push_str(&format!(
            "\nShowing top {} of {} benchmarks.\n",
            max_rows,
            deltas.len()
        ));
    }

    out
}

pub fn emit_github_annotations(deltas: &[BenchmarkDelta], max_items: usize) {
    for d in deltas.iter().take(max_items.max(1)) {
        match d.status {
            RegressionStatus::Fail => {
                println!(
                    "::error::Benchmark regression: {} {:+.2}% (baseline {:.1}ns -> current {:.1}ns)",
                    d.id, d.delta_pct, d.baseline_ns, d.current_ns
                );
            }
            RegressionStatus::Warn => {
                println!(
                    "::warning::Benchmark slowdown: {} {:+.2}% (baseline {:.1}ns -> current {:.1}ns)",
                    d.id, d.delta_pct, d.baseline_ns, d.current_ns
                );
            }
            RegressionStatus::Pass => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_baselines_applies_warn_fail_thresholds() {
        let mut baseline = CriterionBaseline::default();
        baseline.mean_ns.insert("a".into(), 100.0);
        baseline.mean_ns.insert("b".into(), 100.0);
        baseline.mean_ns.insert("c".into(), 100.0);

        let mut current = CriterionBaseline::default();
        current.mean_ns.insert("a".into(), 105.0); // +5%
        current.mean_ns.insert("b".into(), 115.0); // +15% warn
        current.mean_ns.insert("c".into(), 140.0); // +40% fail

        let deltas = compare_baselines(
            &baseline,
            &current,
            ComparisonConfig {
                warn_pct: 10.0,
                fail_pct: 20.0,
            },
        );

        let by_id: std::collections::HashMap<_, _> =
            deltas.iter().map(|d| (d.id.as_str(), d)).collect();

        assert_eq!(by_id["a"].status, RegressionStatus::Pass);
        assert_eq!(by_id["b"].status, RegressionStatus::Warn);
        assert_eq!(by_id["c"].status, RegressionStatus::Fail);
    }

    #[test]
    fn overall_status_prioritizes_fail_over_warn() {
        let deltas = vec![
            BenchmarkDelta {
                id: "a".into(),
                baseline_ns: 1.0,
                current_ns: 1.0,
                delta_pct: 0.0,
                status: RegressionStatus::Pass,
            },
            BenchmarkDelta {
                id: "b".into(),
                baseline_ns: 1.0,
                current_ns: 1.2,
                delta_pct: 20.0,
                status: RegressionStatus::Warn,
            },
        ];
        assert_eq!(overall_status(&deltas), RegressionStatus::Warn);

        let mut deltas2 = deltas.clone();
        deltas2.push(BenchmarkDelta {
            id: "c".into(),
            baseline_ns: 1.0,
            current_ns: 1.5,
            delta_pct: 50.0,
            status: RegressionStatus::Fail,
        });
        assert_eq!(overall_status(&deltas2), RegressionStatus::Fail);
    }
}
