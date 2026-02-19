use crate::cli::args::{InspectArgs, InteractiveArgs, OptimizeArgs, RunArgs};
use crate::debugger::engine::DebuggerEngine;
use crate::runtime::executor::ContractExecutor;
use crate::ui::tui::DebuggerUI;
use crate::Result;
use anyhow::Context;
use std::fs;

/// Execute the run command
pub fn run(args: RunArgs) -> Result<()> {
    println!("Loading contract: {:?}", args.contract);

    // Load WASM file
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    // Parse arguments if provided
    let parsed_args = if let Some(args_json) = &args.args {
        Some(parse_args(args_json)?)
    } else {
        None
    };

    // Parse storage if provided
    let initial_storage = if let Some(storage_json) = &args.storage {
        Some(parse_storage(storage_json)?)
    } else {
        None
    };

    println!("\nStarting debugger...");
    println!("Function: {}", args.function);
    if let Some(ref args) = parsed_args {
        println!("Arguments: {}", args);
    }

    // Create executor
    let mut executor = ContractExecutor::new(wasm_bytes)?;

    // Set up initial storage if provided
    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    // Create debugger engine
    let mut engine = DebuggerEngine::new(executor, args.breakpoint);

    // Execute with debugging
    println!("\n--- Execution Start ---\n");
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    println!("\n--- Execution Complete ---\n");

    println!("Result: {:?}", result);

    Ok(())
}

/// Execute the interactive command
pub fn interactive(args: InteractiveArgs) -> Result<()> {
    println!("Starting interactive debugger for: {:?}", args.contract);

    // Load WASM file
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    // Create executor
    let executor = ContractExecutor::new(wasm_bytes)?;

    // Create debugger engine
    let engine = DebuggerEngine::new(executor, vec![]);

    // Start interactive UI
    println!("\nStarting interactive mode...");
    println!("Type 'help' for available commands\n");

    let mut ui = DebuggerUI::new(engine)?;
    ui.run()?;

    Ok(())
}

/// Execute the inspect command
pub fn inspect(args: InspectArgs) -> Result<()> {
    println!("Inspecting contract: {:?}", args.contract);

    // Load WASM file
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("\nContract Information:");
    println!("  Size: {} bytes", wasm_bytes.len());

    if args.functions {
        println!("\nExported Functions:");
        let functions = crate::utils::wasm::parse_functions(&wasm_bytes)?;
        for func in functions {
            println!("  - {}", func);
        }
    }

    if args.metadata {
        println!("\nMetadata:");
        println!("  (Metadata parsing not yet implemented)");
    }

    Ok(())
}

/// Parse JSON arguments into a string for now (will be improved later)
fn parse_args(json: &str) -> Result<String> {
    // Basic validation
    serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON arguments: {}", json))?;
    Ok(json.to_string())
}

/// Parse JSON storage into a string for now (will be improved later)
fn parse_storage(json: &str) -> Result<String> {
    // Basic validation
    serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON storage: {}", json))?;
    Ok(json.to_string())
}

/// Execute the optimize command
pub fn optimize(args: OptimizeArgs) -> Result<()> {
    println!("Analyzing contract for gas optimization: {:?}", args.contract);

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    let mut executor = ContractExecutor::new(wasm_bytes)?;

    if let Some(storage_json) = &args.storage {
        let storage = parse_storage(storage_json)?;
        executor.set_initial_storage(storage)?;
    }

    let functions_to_analyze = if args.function.is_empty() {
        println!("No functions specified, analyzing all exported functions...");
        crate::utils::wasm::parse_functions(&wasm_bytes)?
    } else {
        args.function.clone()
    };

    let mut optimizer = crate::profiler::analyzer::GasOptimizer::new(executor);

    println!("\nAnalyzing {} function(s)...", functions_to_analyze.len());

    for function_name in &functions_to_analyze {
        println!("  Analyzing function: {}", function_name);
        match optimizer.analyze_function(function_name, args.args.as_deref()) {
            Ok(profile) => {
                println!("    CPU: {} instructions, Memory: {} bytes", 
                    profile.total_cpu, profile.total_memory);
            }
            Err(e) => {
                eprintln!("    Warning: Failed to analyze function {}: {}", function_name, e);
            }
        }
    }

    let contract_path_str = args.contract.to_string_lossy().to_string();
    let report = optimizer.generate_report(&contract_path_str);

    let markdown = optimizer.generate_markdown_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &markdown)
            .with_context(|| format!("Failed to write report to: {:?}", output_path))?;
        println!("\nOptimization report written to: {:?}", output_path);
    } else {
        println!("\n{}", markdown);
    }

    Ok(())
}
