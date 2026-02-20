//! Structured logging utilities for the Soroban debugger.
//!
//! This module provides helper functions and macros for consistent,
//! structured logging across the application using the `tracing` crate.

use std::fmt;

/// Helper function to format and log multi-line output without structured fields.
/// Used for formatted displays like tables and summaries.
pub fn log_display<D: fmt::Display>(message: D, level: LogLevel) {
    let msg = message.to_string();
    match level {
        LogLevel::Info => tracing::info!("{}", msg),
        LogLevel::Warn => tracing::warn!("{}", msg),
        LogLevel::Error => tracing::error!("{}", msg),
        LogLevel::Debug => tracing::debug!("{}", msg),
    }
}

/// Log levels matching tracing crate levels.
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

/// Log contract loading event.
pub fn log_loading_contract(path: &str) {
    tracing::info!(contract = path, "Loading contract");
}

/// Log successful contract load.
pub fn log_contract_loaded(bytes: usize) {
    tracing::info!(bytes, "Contract loaded successfully");
}

/// Log snapshot loading.
pub fn log_loading_snapshot(path: &str) {
    tracing::info!(snapshot = path, "Loading network snapshot");
}

/// Log execution start with optional span.
pub fn log_execution_start(function: &str, arguments: Option<&str>) {
    if let Some(args) = arguments {
        tracing::info!(function, arguments = args, "Starting execution");
    } else {
        tracing::info!(function, "Starting execution");
    }
}

/// Log execution completion with result.
pub fn log_execution_complete(result: &str) {
    tracing::info!(result, "Execution completed");
}

/// Log breakpoint event.
pub fn log_breakpoint(function: &str) {
    tracing::debug!(function, "Breakpoint paused");
}

/// Log storage access.
pub fn log_storage_access(key_count: usize) {
    tracing::debug!(keys = key_count, "Storage accessed");
}

/// Log event emission.
pub fn log_event_emitted(contract_id: &str, topic_count: usize) {
    tracing::debug!(
        contract = contract_id,
        topics = topic_count,
        "Event emitted"
    );
}

/// Log budget usage.
pub fn log_budget_usage(cpu: u64, memory: u64) {
    tracing::debug!(cpu, memory, "Resource budget usage");
}

/// Log analysis operation.
pub fn log_analysis_start(operation: &str) {
    tracing::info!(operation, "Starting analysis");
}

/// Log analysis completion.
pub fn log_analysis_complete(operation: &str, count: usize) {
    tracing::info!(operation, count, "Analysis completed");
}

/// Log optimization result.
pub fn log_optimization_report(path: &str) {
    tracing::info!(path, "Optimization report generated");
}

/// Log high resource usage warning.
pub fn log_high_resource_usage(resource: &str, usage: f64) {
    tracing::warn!(resource, usage, "High resource usage detected");
}

/// Log stepping through execution.
pub fn log_step(step_count: u64) {
    tracing::debug!(step = step_count, "Execution stepped");
}

/// Log debugger interactive mode.
pub fn log_interactive_mode_start() {
    tracing::info!("Interactive debugger started");
}

/// Log breakpoint operations.
pub fn log_breakpoint_set(function: &str) {
    tracing::debug!(function, "Breakpoint set");
}

pub fn log_breakpoint_cleared(function: &str) {
    tracing::debug!(function, "Breakpoint cleared");
}

/// Log repeated execution start.
pub fn log_repeat_execution(function: &str, iterations: usize) {
    tracing::info!(function, iterations, "Starting repeated execution");
}

/// Log comparison between contracts.
pub fn log_contract_comparison(old: &str, new: &str) {
    tracing::info!(old, new, "Comparing contracts");
}
