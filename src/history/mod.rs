use crate::Result;
use anyhow::{Context, Result as AnyhowResult};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunHistory {
    pub date: String,
    pub contract_hash: String,
    pub function: String,
    pub cpu_used: u64,
    pub memory_used: u64,
}

pub struct HistoryManager {
    file_path: PathBuf,
}

impl HistoryManager {
    /// Create a new HistoryManager using the default `~/.soroban-debug/history.json` path.
    pub fn new() -> AnyhowResult<Self> {
        let home_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .context("Could not determine home directory")?;
        let debug_dir = PathBuf::from(home_dir).join(".soroban-debug");
        if !debug_dir.exists() {
            fs::create_dir_all(&debug_dir)?;
        }
        Ok(Self {
            file_path: debug_dir.join("history.json"),
        })
    }

    /// Create a new HistoryManager overriding the base path (for tests).
    pub fn with_path(path: PathBuf) -> Self {
        Self { file_path: path }
    }

    /// Read historical data using highly optimized BufReader.
    pub fn load_history(&self) -> AnyhowResult<Vec<RunHistory>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.file_path)?;
        let reader = BufReader::new(file);
        let history: Vec<RunHistory> =
            serde_json::from_reader(reader).unwrap_or_else(|_| Vec::new());
        Ok(history)
    }

    /// Append a new record optimizing with BufWriter.
    pub fn append_record(&self, record: RunHistory) -> AnyhowResult<()> {
        let mut history = self.load_history()?;
        history.push(record);
        let file = File::create(&self.file_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &history)?;
        Ok(())
    }

    /// Filter historical data based on optional parameters.
    pub fn filter_history(
        &self,
        contract_hash: Option<&str>,
        function: Option<&str>,
    ) -> AnyhowResult<Vec<RunHistory>> {
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
}

/// Calculate the delta between the last two runs. Returns percentage increase if >10%.
pub fn check_regression(records: &[RunHistory]) -> Option<(f64, f64)> {
    if records.len() < 2 {
        return None;
    }
    let latest = &records[records.len() - 1];
    let previous = &records[records.len() - 2];

    let mut regression_cpu = 0.0;
    let mut regression_mem = 0.0;

    if previous.cpu_used > 0 && latest.cpu_used > previous.cpu_used {
        let diff = (latest.cpu_used - previous.cpu_used) as f64;
        let p = (diff / previous.cpu_used as f64) * 100.0;
        if p > 10.0 {
            regression_cpu = p;
        }
    }

    if previous.memory_used > 0 && latest.memory_used > previous.memory_used {
        let diff = (latest.memory_used - previous.memory_used) as f64;
        let p = (diff / previous.memory_used as f64) * 100.0;
        if p > 10.0 {
            regression_mem = p;
        }
    }

    if regression_cpu > 0.0 || regression_mem > 0.0 {
        Some((regression_cpu, regression_mem))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_regression_detection() {
        let p1 = RunHistory {
            date: "prev".into(),
            contract_hash: "hash".into(),
            function: "func".into(),
            cpu_used: 1000,
            memory_used: 1000,
        };
        let p2 = RunHistory {
            date: "latest".into(),
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
    fn test_persistence_logic() {
        let temp = NamedTempFile::new().unwrap();
        let manager = HistoryManager::with_path(temp.path().to_path_buf());

        let record = RunHistory {
            date: "date".into(),
            contract_hash: "hash".into(),
            function: "func".into(),
            cpu_used: 1234,
            memory_used: 5678,
        };

        manager.append_record(record).unwrap();
        let history = manager.load_history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].cpu_used, 1234);
    }
}
