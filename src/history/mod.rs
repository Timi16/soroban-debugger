use crate::{DebuggerError, Result};
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunHistory {
    pub date: String,
    pub contract_hash: String,
    pub function: String,
    pub cpu_used: u64,
    pub memory_used: u64,
}

/// Retention policy controlling how many records to keep and their maximum age.
///
/// Both fields are optional; when `None` that dimension is unconstrained.
/// When both are specified, the stricter constraint (fewer records) wins.
#[derive(Debug, Clone, Default)]
pub struct RetentionPolicy {
    /// Keep only the N most-recent records (by parsed date). Oldest are dropped first.
    pub max_records: Option<usize>,
    /// Drop records whose parsed date is older than `now - max_age_days`.
    pub max_age_days: Option<u64>,
}

impl RetentionPolicy {
    /// Returns `true` if neither constraint is set (no pruning will occur).
    pub fn is_empty(&self) -> bool {
        self.max_records.is_none() && self.max_age_days.is_none()
    }
}

/// Summary returned by [`HistoryManager::prune_history`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneReport {
    pub removed: usize,
    pub remaining: usize,
}

pub struct HistoryManager {
    file_path: PathBuf,
}

fn parse_history_date_to_utc_millis(date: &str) -> Option<i64> {
    let date = date.trim();
    if date.is_empty() {
        return None;
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(date) {
        return Some(dt.with_timezone(&Utc).timestamp_millis());
    }

    if let Ok(dt) = DateTime::parse_from_rfc2822(date) {
        return Some(dt.with_timezone(&Utc).timestamp_millis());
    }

    const FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%m/%d/%Y",
        "%d/%m/%Y %H:%M:%S",
        "%d/%m/%Y %H:%M",
        "%d/%m/%Y",
    ];

    for fmt in FORMATS {
        if fmt.contains("%H") {
            if let Ok(naive) = NaiveDateTime::parse_from_str(date, fmt) {
                return Some(
                    DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp_millis(),
                );
            }
        } else if let Ok(naive) = NaiveDate::parse_from_str(date, fmt) {
            let naive_dt = naive.and_hms_opt(0, 0, 0)?;
            return Some(
                DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc).timestamp_millis(),
            );
        }
    }

    None
}

fn compare_run_history_date(a: &RunHistory, b: &RunHistory) -> Ordering {
    match (
        parse_history_date_to_utc_millis(&a.date),
        parse_history_date_to_utc_millis(&b.date),
    ) {
        (Some(at), Some(bt)) => at.cmp(&bt),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.date.cmp(&b.date),
    }
}

/// Sorts records chronologically (oldest -> newest) using parsed timestamps when possible.
///
/// Records with unparseable dates are sorted after parseable ones, with a stable lexical fallback.
pub fn sort_records_by_date(records: &mut [RunHistory]) {
    records.sort_by(compare_run_history_date);
}

struct HistoryLockGuard {
    lock_path: PathBuf,
}

impl Drop for HistoryLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

impl HistoryManager {
    /// Create a new HistoryManager using the default `~/.soroban-debug/history.json` path.
    pub fn new() -> Result<Self> {
        if let Ok(path) = std::env::var("SOROBAN_DEBUG_HISTORY_FILE") {
            let file_path = PathBuf::from(path);
            if let Some(parent) = file_path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| {
                        DebuggerError::FileError(format!(
                            "Failed to create history directory {:?}: {}",
                            parent, e
                        ))
                    })?;
                }
            }
            return Ok(Self { file_path });
        }

        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                DebuggerError::FileError("Could not determine home directory".to_string())
            })?;
        let debug_dir = PathBuf::from(home_dir).join(".soroban-debug");
        if !debug_dir.exists() {
            fs::create_dir_all(&debug_dir).map_err(|e| {
                DebuggerError::FileError(format!(
                    "Failed to create debug directory {:?}: {}",
                    debug_dir, e
                ))
            })?;
        }
        Ok(Self {
            file_path: debug_dir.join("history.json"),
        })
    }

    /// Create a new HistoryManager overriding the base path (for tests).
    pub fn with_path(path: PathBuf) -> Self {
        Self { file_path: path }
    }

    /// Read historical run data from disk.
    ///
    /// Returns `Err` if the history file exists but cannot be parsed.
    ///
    /// # Why we no longer silently return an empty `Vec`
    ///
    /// The previous implementation called
    /// `serde_json::from_reader(reader).unwrap_or_else(|_| Vec::new())`.
    /// That had two severe consequences:
    ///
    /// 1. **Silent data loss** — a corrupt or partially-written file appeared
    ///    identical to a brand-new installation.  Callers such as
    ///    `budget_trend_stats` and `check_regression` would operate on zero
    ///    records and produce misleading (or absent) output without any
    ///    indication that real data had been ignored.
    ///
    /// 2. **Destructive overwrite** — `append_record` calls `load_history`
    ///    before writing.  With the silent fallback it would receive an empty
    ///    `Vec`, push one new record, and atomically replace the corrupt file
    ///    with a single-record file, permanently destroying all prior history.
    ///
    /// Surfacing the error lets the caller decide on a recovery strategy and
    /// prevents any write path from silently clobbering salvageable data.
    pub fn load_history(&self) -> Result<Vec<RunHistory>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.file_path).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to open history file {:?}: {}",
                self.file_path, e
            ))
        })?;

        let reader = BufReader::new(file);

        // Surface parse failures rather than swallowing them.
        //
        // A corrupt or truncated file must not be treated as an empty history.
        // Doing so would cause `append_record` to overwrite the file with a
        // single-record list, destroying all salvageable data.
        let history: Vec<RunHistory> = serde_json::from_reader(reader).map_err(|e| {
            DebuggerError::FileError(format!(
                "History file \"{}\" could not be parsed ({}). \
                 The file may be corrupt or was written by an incompatible version. \
                 Recovery options:\n\
                 \x20 1. Inspect the file with `cat \"{}\"` and fix any JSON syntax errors.\n\
                 \x20 2. Back up and remove the file (`mv \"{}\" \"{}.bak\"`) to start fresh.\n\
                 \x20 3. Restore from a previous backup if one exists.",
                self.file_path.display(),
                e,
                self.file_path.display(),
                self.file_path.display(),
                self.file_path.display(),
            ))
        })?;
        Ok(history)
    }

    /// Append a new record optimizing with BufWriter.
    ///
    /// No retention policy is applied. Use [`append_record_with_policy`] to
    /// automatically prune after appending.
    pub fn append_record(&self, record: RunHistory) -> Result<()> {
        self.append_record_with_policy(record, &RetentionPolicy::default())
    }

    /// Append a new record and apply `policy` before flushing to disk.
    ///
    /// The sequence is:
    /// 1. Acquire the file lock.
    /// 2. Load current history.
    /// 3. Push the new record.
    /// 4. Apply retention (sort → prune).
    /// 5. Atomically replace the file.
    pub fn append_record_with_policy(
        &self,
        record: RunHistory,
        policy: &RetentionPolicy,
    ) -> Result<()> {
        let _lock = self.acquire_lock()?;
        let mut history = self.load_history()?;
        history.push(record);

        if !policy.is_empty() {
            Self::apply_retention(&mut history, policy);
        }

        self.flush_history(&history)
    }

    /// Prune the history file according to `policy` and return a [`PruneReport`].
    ///
    /// The operation is atomic: a temp file is written and renamed, so a
    /// crash mid-write cannot corrupt the existing history.
    pub fn prune_history(&self, policy: &RetentionPolicy) -> Result<PruneReport> {
        let _lock = self.acquire_lock()?;
        let mut history = self.load_history()?;
        let before = history.len();

        Self::apply_retention(&mut history, policy);
        let remaining = history.len();
        let removed = before - remaining;

        if removed > 0 {
            self.flush_history(&history)?;
        }

        Ok(PruneReport { removed, remaining })
    }

    /// Apply `policy` to `records` in-place.
    ///
    /// Records are first sorted chronologically (oldest → newest). The age
    /// filter drops records whose date is older than `now - max_age_days`.
    /// The count filter keeps only the trailing `max_records` entries.
    ///
    /// Deterministic ordering is preserved for callers that later run
    /// regression or trend analysis on the pruned slice.
    pub fn apply_retention(records: &mut Vec<RunHistory>, policy: &RetentionPolicy) {
        if policy.is_empty() {
            return;
        }

        // Sort oldest-to-newest so we can trim from the front.
        sort_records_by_date(records);

        // Age filter: drop records older than `now - max_age_days`.
        if let Some(max_age) = policy.max_age_days {
            let cutoff = Utc::now() - ChronoDuration::days(max_age as i64);
            let cutoff_ms = cutoff.timestamp_millis();
            records.retain(|r| {
                // If date cannot be parsed, keep the record (safe default).
                match parse_history_date_to_utc_millis(&r.date) {
                    Some(ms) => ms >= cutoff_ms,
                    None => true,
                }
            });
        }

        // Count filter: keep only the N most-recent (tail of sorted list).
        if let Some(max_n) = policy.max_records {
            let len = records.len();
            if len > max_n {
                records.drain(..len - max_n);
            }
        }
    }

    // ── private helpers ─────────────────────────────────────────────────────

    /// Write `history` to disk atomically: tmp file → fsync → rename.
    fn flush_history(&self, history: &[RunHistory]) -> Result<()> {
        let tmp_path = self.file_path.with_extension("json.tmp");
        let file = File::create(&tmp_path).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to create temp history file {:?}: {}",
                tmp_path, e
            ))
        })?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, history).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write history file {:?}: {}",
                self.file_path, e
            ))
        })?;
        writer.flush().map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to flush temp history file {:?}: {}",
                tmp_path, e
            ))
        })?;
        if let Ok(file) = writer.into_inner() {
            let _ = file.sync_all();
        }

        if self.file_path.exists() {
            let _ = fs::remove_file(&self.file_path);
        }
        fs::rename(&tmp_path, &self.file_path).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to replace history file {:?}: {}",
                self.file_path, e
            ))
        })?;
        Ok(())
    }

    /// Filter historical data based on optional parameters.
    pub fn filter_history(
        &self,
        contract_hash: Option<&str>,
        function: Option<&str>,
    ) -> Result<Vec<RunHistory>> {
        let history = self.load_history()?;
        let filtered = history
            .into_iter()
            .filter(|r| {
                let match_contract = match contract_hash {
                    Some(c) => r.contract_hash == c,
                    None => true,
                };
                let match_function = match function {
                    Some(f) => r.function == f,
                    None => true,
                };
                match_contract && match_function
            })
            .collect();
        Ok(filtered)
    }

    fn acquire_lock(&self) -> Result<HistoryLockGuard> {
        let lock_path = self.file_path.with_extension("lock");
        let start = SystemTime::now();

        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut f) => {
                    let _ = writeln!(f, "pid={}", std::process::id());
                    return Ok(HistoryLockGuard { lock_path });
                }
                Err(_) => {
                    // If a previous process crashed, allow breaking a stale lock after a grace period.
                    if let Ok(meta) = fs::metadata(&lock_path) {
                        if let Ok(modified) = meta.modified() {
                            if modified
                                .elapsed()
                                .unwrap_or(Duration::from_secs(0))
                                .as_secs()
                                > 30
                            {
                                let _ = fs::remove_file(&lock_path);
                                continue;
                            }
                        }
                    }

                    if start.elapsed().unwrap_or(Duration::from_secs(0)).as_secs() > 30 {
                        return Err(DebuggerError::FileError(format!(
                            "Timed out waiting for history lock at {:?}",
                            lock_path
                        ))
                        .into());
                    }

                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}

/// Calculate the delta between the last two runs. Returns percentage increase if >10%.
pub fn check_regression(records: &[RunHistory]) -> Option<(f64, f64)> {
    check_regression_with_config(records, &RegressionConfig::default())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegressionConfig {
    /// Percentage threshold above which we consider a regression.
    ///
    /// Example: `10.0` means "warn if latest is >10% higher than baseline".
    pub threshold_pct: f64,
    /// Number of most-recent records to consider for regression detection.
    ///
    /// The baseline is computed from the previous `lookback - 1` runs, and compared to the latest.
    pub lookback: usize,
    /// Trailing smoothing window size (moving average) applied to the series before regression detection.
    ///
    /// `1` disables smoothing.
    pub smoothing_window: usize,
}

impl Default for RegressionConfig {
    fn default() -> Self {
        Self {
            threshold_pct: 10.0,
            lookback: 2,
            smoothing_window: 1,
        }
    }
}

fn smooth_trailing(values: &[u64], window: usize) -> Vec<f64> {
    let window = window.max(1);
    let mut out = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_sub(window - 1);
        let mut sum: u128 = 0;
        let mut count: u128 = 0;
        for v in &values[start..=i] {
            sum = sum.saturating_add(*v as u128);
            count += 1;
        }
        out.push((sum as f64) / (count as f64));
    }
    out
}

/// Check for CPU and memory regressions using a configurable lookback window and smoothing.
///
/// Returns `(cpu_pct, mem_pct)` where each value is `> 0.0` only when it exceeds `threshold_pct`.
pub fn check_regression_with_config(
    records: &[RunHistory],
    config: &RegressionConfig,
) -> Option<(f64, f64)> {
    if records.len() < 2 {
        return None;
    }

    let lookback = config.lookback.max(2);
    let smoothing = config.smoothing_window.max(1);
    let threshold = config.threshold_pct.max(0.0);

    let mut sorted: Vec<&RunHistory> = records.iter().collect();
    sorted.sort_by(|a, b| compare_run_history_date(a, b));

    let window_len = sorted.len().min(lookback);
    if window_len < 2 {
        return None;
    }

    let window = &sorted[sorted.len() - window_len..];
    let cpu_raw: Vec<u64> = window.iter().map(|r| r.cpu_used).collect();
    let mem_raw: Vec<u64> = window.iter().map(|r| r.memory_used).collect();
    let cpu = smooth_trailing(&cpu_raw, smoothing);
    let mem = smooth_trailing(&mem_raw, smoothing);

    let cpu_latest = cpu[cpu.len() - 1];
    let mem_latest = mem[mem.len() - 1];

    let cpu_baseline = cpu[..cpu.len() - 1].iter().sum::<f64>() / ((cpu.len() - 1) as f64);
    let mem_baseline = mem[..mem.len() - 1].iter().sum::<f64>() / ((mem.len() - 1) as f64);

    let mut regression_cpu = 0.0;
    let mut regression_mem = 0.0;

    if cpu_baseline > 0.0 && cpu_latest > cpu_baseline {
        let p = ((cpu_latest - cpu_baseline) / cpu_baseline) * 100.0;
        if p > threshold {
            regression_cpu = p;
        }
    }

    if mem_baseline > 0.0 && mem_latest > mem_baseline {
        let p = ((mem_latest - mem_baseline) / mem_baseline) * 100.0;
        if p > threshold {
            regression_mem = p;
        }
    }

    if regression_cpu > 0.0 || regression_mem > 0.0 {
        Some((regression_cpu, regression_mem))
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct BudgetTrendStats {
    pub count: usize,
    pub first_date: String,
    pub last_date: String,
    pub cpu_min: u64,
    pub cpu_avg: u64,
    pub cpu_max: u64,
    pub mem_min: u64,
    pub mem_avg: u64,
    pub mem_max: u64,
    pub last_cpu: u64,
    pub last_mem: u64,
}

pub fn budget_trend_stats(records: &[RunHistory]) -> Option<BudgetTrendStats> {
    if records.is_empty() {
        return None;
    }

    let mut cpu_min = u64::MAX;
    let mut cpu_max = 0u64;
    let mut mem_min = u64::MAX;
    let mut mem_max = 0u64;
    let mut cpu_sum: u128 = 0;
    let mut mem_sum: u128 = 0;

    for r in records {
        cpu_min = cpu_min.min(r.cpu_used);
        cpu_max = cpu_max.max(r.cpu_used);
        mem_min = mem_min.min(r.memory_used);
        mem_max = mem_max.max(r.memory_used);
        cpu_sum = cpu_sum.saturating_add(r.cpu_used as u128);
        mem_sum = mem_sum.saturating_add(r.memory_used as u128);
    }

    let mut sorted: Vec<&RunHistory> = records.iter().collect();
    sorted.sort_by(|a, b| compare_run_history_date(a, b));
    let count = sorted.len();
    let first = sorted[0];
    let last = sorted[count - 1];

    Some(BudgetTrendStats {
        count,
        first_date: first.date.clone(),
        last_date: last.date.clone(),
        cpu_min,
        cpu_avg: (cpu_sum / count as u128) as u64,
        cpu_max,
        mem_min,
        mem_avg: (mem_sum / count as u128) as u64,
        mem_max,
        last_cpu: last.cpu_used,
        last_mem: last.memory_used,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::io::Write as IoWrite;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    // ── helpers ─────────────────────────────────────────────────────────────

    fn make_record(date: &str, cpu: u64, mem: u64) -> RunHistory {
        RunHistory {
            date: date.into(),
            contract_hash: "hash".into(),
            function: "func".into(),
            cpu_used: cpu,
            memory_used: mem,
        }
    }

    // ── load_history — corrupt file tests (the requested verification) ───────

    /// A file containing invalid JSON must return `Err`, never `Ok(vec![])`.
    #[test]
    fn load_history_corrupt_file_returns_error() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"this is not json at all {{{").unwrap();
        tmp.flush().unwrap();

        let manager = HistoryManager::with_path(tmp.path().to_path_buf());
        let result = manager.load_history();

        assert!(
            result.is_err(),
            "corrupt file must return Err, not Ok(vec![])"
        );
    }

    /// The error message must reference the file path so the user knows which
    /// file to inspect.
    #[test]
    fn load_history_error_message_contains_file_path() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"{not valid json}").unwrap();
        tmp.flush().unwrap();

        let path = tmp.path().to_path_buf();
        let manager = HistoryManager::with_path(path.clone());
        let err = manager.load_history().unwrap_err();
        let msg = err.to_string();

        assert!(
            msg.contains(path.to_string_lossy().as_ref()),
            "error must mention the file path; got: {msg}"
        );
    }

    /// The error message must contain recovery guidance so users know what
    /// action to take without having to read source code.
    #[test]
    fn load_history_error_message_contains_recovery_guidance() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"[{\"broken\":").unwrap(); // truncated JSON
        tmp.flush().unwrap();

        let manager = HistoryManager::with_path(tmp.path().to_path_buf());
        let err = manager.load_history().unwrap_err();
        let msg = err.to_string();

        // The message must mention at least one concrete recovery action.
        let has_guidance = msg.contains("bak")      // backup suggestion
            || msg.contains("fix")                  // fix-in-place suggestion
            || msg.contains("Inspect")              // inspect suggestion
            || msg.contains("Recovery"); // recovery header

        assert!(
            has_guidance,
            "error must include recovery guidance; got: {msg}"
        );
    }

    /// A truncated (partial write) JSON array must also be treated as corrupt
    /// and return `Err`.
    #[test]
    fn load_history_truncated_file_returns_error() {
        let mut tmp = NamedTempFile::new().unwrap();
        // Valid start but cut off mid-record.
        tmp.write_all(b"[{\"date\":\"2026-01-01\",\"contract_hash\":\"a\"")
            .unwrap();
        tmp.flush().unwrap();

        let manager = HistoryManager::with_path(tmp.path().to_path_buf());
        assert!(
            manager.load_history().is_err(),
            "truncated file must return Err"
        );
    }

    /// A file containing a JSON object (not an array) must return `Err`
    /// because the expected type is `Vec<RunHistory>`.
    #[test]
    fn load_history_wrong_json_type_returns_error() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"{\"date\":\"2026\",\"cpu_used\":1}")
            .unwrap();
        tmp.flush().unwrap();

        let manager = HistoryManager::with_path(tmp.path().to_path_buf());
        assert!(
            manager.load_history().is_err(),
            "a JSON object instead of array must return Err"
        );
    }

    /// A non-existent file must still return `Ok(vec![])` (first-run case).
    #[test]
    fn load_history_missing_file_returns_empty_ok() {
        let path = std::env::temp_dir().join("soroban-history-definitely-does-not-exist.json");
        let _ = fs::remove_file(&path); // ensure absent
        let manager = HistoryManager::with_path(path);
        let result = manager.load_history();
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    /// A valid empty JSON array must parse successfully and return `Ok(vec![])`.
    #[test]
    fn load_history_empty_array_returns_ok() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"[]").unwrap();
        tmp.flush().unwrap();

        let manager = HistoryManager::with_path(tmp.path().to_path_buf());
        let result = manager.load_history().unwrap();
        assert!(result.is_empty());
    }

    /// `append_record` must propagate the error rather than silently overwriting
    /// a corrupt file with a single-record list.
    #[test]
    fn append_record_does_not_overwrite_corrupt_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        let corrupt_content = b"this is corrupt {{{";
        tmp.write_all(corrupt_content).unwrap();
        tmp.flush().unwrap();
        let path = tmp.path().to_path_buf();

        let manager = HistoryManager::with_path(path.clone());
        let new_record = make_record("2026-01-01T00:00:00Z", 100, 200);

        let result = manager.append_record(new_record);
        assert!(
            result.is_err(),
            "append_record must fail on a corrupt history file, not silently overwrite it"
        );

        // The original corrupt content must still be on disk — not replaced.
        let on_disk = fs::read(&path).unwrap();
        assert_eq!(
            on_disk, corrupt_content,
            "corrupt file must not be modified by a failed append"
        );
    }

    // ── pre-existing tests (unchanged) ───────────────────────────────────────

    #[test]
    fn test_regression_detection() {
        let p1 = make_record("2026-01-01T00:00:00Z", 1000, 1000);
        let p2 = RunHistory {
            date: "2026-01-02T00:00:00Z".into(),
            contract_hash: "hash".into(),
            function: "func".into(),
            cpu_used: 1150,    // 15% increase
            memory_used: 1050, // 5% increase
        };

        let records = vec![p1, p2];
        let regression = check_regression(&records);
        assert!(regression.is_some());
        let (cpu, mem) = regression.unwrap();
        assert_eq!(cpu, 15.0);
        assert_eq!(mem, 0.0);
    }

    #[test]
    fn regression_threshold_is_configurable() {
        let p1 = make_record("2026-01-01T00:00:00Z", 1000, 1000);
        let p2 = make_record("2026-01-02T00:00:00Z", 1150, 1050); // +15% cpu, +5% mem
        let records = vec![p1, p2];

        let cfg = RegressionConfig {
            threshold_pct: 20.0,
            lookback: 2,
            smoothing_window: 1,
        };
        assert!(check_regression_with_config(&records, &cfg).is_none());
    }

    #[test]
    fn regression_lookback_window_changes_baseline() {
        // With lookback=2: baseline=140 -> latest=150 => ~7.14% (no regression for threshold 10%)
        // With lookback=4: baseline=avg(100,120,140)=120 -> latest=150 => 25% (regression)
        let records = vec![
            make_record("2026-01-01", 100, 100),
            make_record("2026-01-02", 120, 100),
            make_record("2026-01-03", 140, 100),
            make_record("2026-01-04", 150, 100),
        ];

        let cfg_short = RegressionConfig {
            threshold_pct: 10.0,
            lookback: 2,
            smoothing_window: 1,
        };
        assert!(check_regression_with_config(&records, &cfg_short).is_none());

        let cfg_long = RegressionConfig {
            threshold_pct: 10.0,
            lookback: 4,
            smoothing_window: 1,
        };
        let (cpu, mem) = check_regression_with_config(&records, &cfg_long).unwrap();
        assert!(cpu > 20.0 && cpu < 30.0, "expected ~25%, got {cpu}");
        assert_eq!(mem, 0.0);
    }

    #[test]
    fn regression_smoothing_can_reduce_noise() {
        // Without smoothing, latest=200 vs baseline avg(100,200,100)=133.33 => 50% regression.
        // With smoothing_window=3, the smoothed baseline/last reduce the delta below 40%.
        let records = vec![
            make_record("2026-01-01", 100, 0),
            make_record("2026-01-02", 200, 0),
            make_record("2026-01-03", 100, 0),
            make_record("2026-01-04", 200, 0),
        ];

        let cfg_raw = RegressionConfig {
            threshold_pct: 40.0,
            lookback: 4,
            smoothing_window: 1,
        };
        assert!(check_regression_with_config(&records, &cfg_raw).is_some());

        let cfg_smooth = RegressionConfig {
            threshold_pct: 40.0,
            lookback: 4,
            smoothing_window: 3,
        };
        assert!(check_regression_with_config(&records, &cfg_smooth).is_none());
    }

    #[test]
    fn test_persistence_logic() {
        let temp = tempfile::tempdir().unwrap();
        let manager = HistoryManager::with_path(temp.path().join("history.json"));

        let record = make_record("2026-01-01T00:00:00Z", 1234, 5678);
        manager.append_record(record).unwrap();
        let history = manager.load_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].cpu_used, 1234);
    }

    #[test]
    fn budget_trend_stats_empty_returns_none() {
        assert!(budget_trend_stats(&[]).is_none());
    }

    #[test]
    fn sort_records_by_date_handles_mixed_formats() {
        let mut records = vec![
            make_record("01/02/2026 00:00:00", 1, 1),
            make_record("2026-01-01T00:00:00Z", 1, 1),
            make_record("2026-01-03", 1, 1),
        ];

        sort_records_by_date(&mut records);
        assert_eq!(records[0].date, "2026-01-01T00:00:00Z");
        assert_eq!(records[1].date, "01/02/2026 00:00:00");
        assert_eq!(records[2].date, "2026-01-03");
    }

    #[test]
    fn budget_trend_stats_uses_parsed_date_order_for_first_last() {
        let records = vec![
            make_record("01/02/2026 00:00:00", 10, 10),
            make_record("2026-01-01T00:00:00Z", 20, 20),
        ];

        let stats = budget_trend_stats(&records).unwrap();
        assert_eq!(stats.first_date, "2026-01-01T00:00:00Z");
        assert_eq!(stats.last_date, "01/02/2026 00:00:00");
    }

    #[test]
    fn check_regression_uses_parsed_date_order_for_latest_two() {
        let records = vec![
            make_record("2026-01-02T00:00:00Z", 100, 100),
            make_record("01/03/2026 00:00:00", 120, 100),
            make_record("2026-01-01", 80, 100),
        ];

        let (cpu, mem) = check_regression(&records).unwrap();
        assert_eq!(cpu, 20.0);
        assert_eq!(mem, 0.0);
    }

    #[test]
    fn concurrent_append_preserves_all_records() {
        let temp = TempDir::new().unwrap();
        let history_path = temp.path().join("history.json");
        let manager = std::sync::Arc::new(HistoryManager::with_path(history_path));

        let threads = 16usize;
        let per_thread = 25usize;
        let mut handles = Vec::new();

        for t in 0..threads {
            let manager = std::sync::Arc::clone(&manager);
            handles.push(std::thread::spawn(move || {
                for i in 0..per_thread {
                    let record = RunHistory {
                        date: format!("t{t}-i{i}"),
                        contract_hash: "hash".into(),
                        function: "func".into(),
                        cpu_used: (t as u64) * 10 + i as u64,
                        memory_used: (t as u64) * 10 + i as u64,
                    };
                    manager.append_record(record).unwrap();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let history = manager.load_history().unwrap();
        assert_eq!(history.len(), threads * per_thread);
    }

    #[test]
    fn new_respects_history_file_env_override() {
        let temp = TempDir::new().unwrap();
        let history_path = temp.path().join("custom").join("history.json");

        let old = std::env::var("SOROBAN_DEBUG_HISTORY_FILE").ok();
        std::env::set_var("SOROBAN_DEBUG_HISTORY_FILE", &history_path);

        let manager = HistoryManager::new().unwrap();
        manager
            .append_record(RunHistory {
                date: "d".into(),
                contract_hash: "h".into(),
                function: "f".into(),
                cpu_used: 1,
                memory_used: 1,
            })
            .unwrap();

        assert!(history_path.exists());

        if let Some(old) = old {
            std::env::set_var("SOROBAN_DEBUG_HISTORY_FILE", old);
        } else {
            std::env::remove_var("SOROBAN_DEBUG_HISTORY_FILE");
        }
    }

    #[test]
    fn budget_trend_stats_computes_min_max_avg_last() {
        let records = vec![
            make_record("2026-01-01T00:00:00Z", 10, 100),
            make_record("2026-01-02T00:00:00Z", 30, 200),
            make_record("2026-01-03T00:00:00Z", 20, 150),
        ];

        let stats = budget_trend_stats(&records).unwrap();
        assert_eq!(stats.count, 3);
        assert_eq!(stats.cpu_min, 10);
        assert_eq!(stats.cpu_max, 30);
        assert_eq!(stats.cpu_avg, 20);
        assert_eq!(stats.mem_min, 100);
        assert_eq!(stats.mem_max, 200);
        assert_eq!(stats.mem_avg, 150);
        assert_eq!(stats.last_cpu, 20);
        assert_eq!(stats.last_mem, 150);
        assert_eq!(stats.first_date, "2026-01-01T00:00:00Z");
        assert_eq!(stats.last_date, "2026-01-03T00:00:00Z");
    }

    // ── RetentionPolicy / apply_retention tests ──────────────────────────────

    #[test]
    fn apply_retention_noop_when_policy_is_empty() {
        let mut records = vec![
            make_record("2026-01-01T00:00:00Z", 1, 1),
            make_record("2026-01-02T00:00:00Z", 2, 2),
        ];
        let policy = RetentionPolicy::default();
        HistoryManager::apply_retention(&mut records, &policy);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn apply_retention_max_records_keeps_newest() {
        let mut records = vec![
            make_record("2026-01-01T00:00:00Z", 1, 1),
            make_record("2026-01-02T00:00:00Z", 2, 2),
            make_record("2026-01-03T00:00:00Z", 3, 3),
            make_record("2026-01-04T00:00:00Z", 4, 4),
            make_record("2026-01-05T00:00:00Z", 5, 5),
        ];
        let policy = RetentionPolicy {
            max_records: Some(3),
            max_age_days: None,
        };
        HistoryManager::apply_retention(&mut records, &policy);
        assert_eq!(records.len(), 3);
        // oldest two are gone; newest three remain
        assert_eq!(records[0].cpu_used, 3);
        assert_eq!(records[2].cpu_used, 5);
    }

    #[test]
    fn apply_retention_max_records_noop_when_under_limit() {
        let mut records = vec![
            make_record("2026-01-01T00:00:00Z", 1, 1),
            make_record("2026-01-02T00:00:00Z", 2, 2),
        ];
        let policy = RetentionPolicy {
            max_records: Some(10),
            max_age_days: None,
        };
        HistoryManager::apply_retention(&mut records, &policy);
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn apply_retention_max_age_drops_old_records() {
        // Build a record 10 days in the past and one 1 day in the past.
        let old_date = (Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        let recent_date = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        let mut records = vec![
            make_record(&old_date, 1, 1),
            make_record(&recent_date, 2, 2),
        ];
        let policy = RetentionPolicy {
            max_records: None,
            max_age_days: Some(5),
        };
        HistoryManager::apply_retention(&mut records, &policy);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].cpu_used, 2);
    }

    #[test]
    fn apply_retention_unparseable_date_is_kept() {
        let mut records = vec![
            make_record("not-a-date", 99, 99),
            make_record("2026-01-01T00:00:00Z", 1, 1),
        ];
        // Drop anything older than 1 day; "not-a-date" should be preserved.
        let policy = RetentionPolicy {
            max_records: None,
            max_age_days: Some(1),
        };
        HistoryManager::apply_retention(&mut records, &policy);
        let cpus: Vec<u64> = records.iter().map(|r| r.cpu_used).collect();
        assert!(
            cpus.contains(&99),
            "record with unparseable date must not be dropped"
        );
    }

    #[test]
    fn apply_retention_combined_policy_applies_both_constraints() {
        let old_date = (Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        let mut records = vec![
            make_record(&old_date, 1, 1),                // dropped by age
            make_record("2026-01-04T00:00:00Z", 10, 10), // parseable but static past date
            make_record("2026-01-05T00:00:00Z", 20, 20),
            make_record("2026-01-06T00:00:00Z", 30, 30),
        ];
        // max_age_days=5 drops the 10-day-old record; max_records=2 then keeps newest 2.
        let policy = RetentionPolicy {
            max_records: Some(2),
            max_age_days: Some(5),
        };
        HistoryManager::apply_retention(&mut records, &policy);
        // After age-filter: 3 records remain (old_date dropped, static dates kept because
        // they parse as recent relative to each other but are in the past beyond our cutoff).
        // Regardless, max_records=2 must hold.
        assert!(
            records.len() <= 2,
            "max_records must be respected; got {}",
            records.len()
        );
    }

    #[test]
    fn prune_history_returns_correct_report() {
        let temp = TempDir::new().unwrap();
        let manager = HistoryManager::with_path(temp.path().join("history.json"));
        for i in 1..=5 {
            manager
                .append_record(make_record(&format!("2026-01-0{}T00:00:00Z", i), i, i))
                .unwrap();
        }

        let policy = RetentionPolicy {
            max_records: Some(3),
            max_age_days: None,
        };
        let report = manager.prune_history(&policy).unwrap();
        assert_eq!(report.removed, 2);
        assert_eq!(report.remaining, 3);

        let history = manager.load_history().unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn prune_history_no_write_when_nothing_removed() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("history.json");
        let manager = HistoryManager::with_path(path.clone());
        manager
            .append_record(make_record("2026-01-01T00:00:00Z", 1, 1))
            .unwrap();

        let mtime_before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        let policy = RetentionPolicy {
            max_records: Some(10),
            max_age_days: None,
        };
        let report = manager.prune_history(&policy).unwrap();
        assert_eq!(report.removed, 0);

        // File should NOT have been rewritten (mtime unchanged).
        let mtime_after = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "file must not be rewritten when nothing is removed"
        );
    }

    #[test]
    fn append_record_with_policy_prunes_oldest() {
        let temp = TempDir::new().unwrap();
        let manager = HistoryManager::with_path(temp.path().join("history.json"));

        let policy = RetentionPolicy {
            max_records: Some(2),
            max_age_days: None,
        };

        for i in 1..=4u64 {
            manager
                .append_record_with_policy(
                    make_record(&format!("2026-01-0{}T00:00:00Z", i), i, i),
                    &policy,
                )
                .unwrap();
        }

        let history = manager.load_history().unwrap();
        assert_eq!(history.len(), 2, "should have pruned to max 2 records");
        // The two most-recent (day 3 and 4) should survive.
        let cpus: Vec<u64> = history.iter().map(|r| r.cpu_used).collect();
        assert!(cpus.contains(&3));
        assert!(cpus.contains(&4));
    }
}
