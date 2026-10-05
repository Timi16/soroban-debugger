//! Runtime execution sub-system for the Soroban debugger.
//!
//! Sub-modules:
//! - [`executor`]       — Public façade; coordinates all sub-modules.
//! - [`loader`]         — WASM loading and Soroban environment bootstrap.
//! - [`invoker`]        — Contract function invocation with timeout protection.
//! - [`parser`]         — Argument parsing and type-aware JSON normalisation.
//! - [`result`]         — Shared result types and formatting helpers.
//! - [`env`]            — Debug environment utilities.
//! - [`instruction`]    — WASM instruction parsing.
//! - [`instrumentation`]— Instruction-level hooks for profiling.
//! - [`mocking`]        — Mock contract registry and dispatcher.

pub mod env;
pub mod executor;
pub mod instruction;
pub mod instrumentation;
pub mod invoker;
pub mod loader;
pub mod mocking;
pub mod parser;
pub mod result;

// Top-level re-exports — public API is unchanged.
pub use env::DebugEnv;
pub use executor::ContractExecutor;
pub use executor::{ExecutionRecord, InstructionCounts, MockCallEntry, StorageSnapshot};
pub use instruction::{Instruction, InstructionParser, OwnedOperator};
pub use instrumentation::{InstructionHook, Instrumenter};
