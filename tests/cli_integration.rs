#![cfg(any())]
/// End-to-end CLI integration tests
///
/// This module orchestrates all CLI integration tests for the soroban-debug binary.
/// It provides comprehensive test coverage for:
/// - Command help and version information
/// - Error handling and validation
/// - Output format options and JSON validity
/// - All major CLI subcommands
#[path = "cli/common.rs"]
mod common;

#[path = "cli/help_tests.rs"]
mod help_tests;

#[path = "cli/error_tests.rs"]
mod error_tests;

#[path = "cli/output_tests.rs"]
mod output_tests;
