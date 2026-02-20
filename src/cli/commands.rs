use crate::cli::args::{CompareArgs, InspectArgs, InteractiveArgs, OptimizeArgs, RunArgs, UpgradeCheckArgs};
use crate::debugger::engine::DebuggerEngine;
use crate::repeat::RepeatRunner;
use crate::runtime::executor::ContractExecutor;
use crate::simulator::SnapshotLoader;
use crate::ui::tui::DebuggerUI;
use crate::Result;
use anyhow::Context;
use std::fs;
use tracing::info as log_info;

/// Execute the run command
pub fn run(args: RunArgs) -> Result<()> {
    println!("Loading contract: {:?}", args.contract);

    // Load WASM file
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    // Load network snapshot if provided
    if let Some(snapshot_path) = &args.network_snapshot {
        println!("\nLoading network snapshot: {:?}", snapshot_path);
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        println!("{}", loaded_snapshot.format_summary());
    }

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

    // Handle --repeat N: run N times and show aggregate stats
    if let Some(n) = args.repeat {
        let runner = RepeatRunner::new(wasm_bytes, args.breakpoint, initial_storage);
        let stats = runner.run(&args.function, parsed_args.as_deref(), n)?;
        stats.display();
        return Ok(());
    }

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

    // Display events if requested
    if args.show_events {
        println!("\n--- Events ---");
        let events = engine.executor().get_events()?;
        let filtered_events = if let Some(topic) = &args.filter_topic {
            crate::inspector::events::EventInspector::filter_events(&events, topic)
        } else {
            events
        };

        if filtered_events.is_empty() {
            println!("No events captured.");
        } else {
            for (i, event) in filtered_events.iter().enumerate() {
                println!("Event #{}:", i);
                if let Some(contract_id) = &event.contract_id {
                    println!("  Contract: {}", contract_id);
                }
                println!("  Topics: {:?}", event.topics);
                println!("  Data: {}", event.data);
                println!();
            }
        }
    }

    // Display storage with optional filtering
    if !args.storage_filter.is_empty() {
        let storage_filter = crate::inspector::storage::StorageFilter::new(&args.storage_filter)
            .map_err(|e| anyhow::anyhow!("Invalid storage filter: {}", e))?;
        println!("\n--- Storage ---");
        let inspector = crate::inspector::StorageInspector::new();
        inspector.display_filtered(&storage_filter);
    }

    // If output format is JSON, print full result as JSON and exit
    if let Some(format) = &args.format {
        if format.eq_ignore_ascii_case("json") {
            let mut output = serde_json::json!({
                "result": format!("{:?}", result),
            });

            if args.show_events {
                let events = engine.executor().get_events()?;
                output["events"] = serde_json::to_value(&events).unwrap_or(serde_json::Value::Null);
            }

            println!("{}", serde_json::to_string_pretty(&output).unwrap());
            return Ok(());
        }
    }

    Ok(())
}

/// Execute the interactive command
pub fn interactive(args: InteractiveArgs) -> Result<()> {
    println!("Starting interactive debugger for: {:?}", args.contract);

    // Load WASM file
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    // Load network snapshot if provided
    if let Some(snapshot_path) = &args.network_snapshot {
        println!("\nLoading network snapshot: {:?}", snapshot_path);
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        println!("{}", loaded_snapshot.format_summary());
    }

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

/// Parse JSON arguments with validation (actual parsing happens during execution)
pub fn parse_args(json: &str) -> Result<String> {
    // Validate JSON structure at parse time to give quick feedback
    let value = serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON arguments: {}", json))?;

    // Provide helpful context about what was parsed
    match value {
        serde_json::Value::Array(ref arr) => {
            log_info!("Parsed {} argument(s)", arr.len());
        }
        serde_json::Value::Object(ref obj) => {
            log_info!("Parsed object with {} fields for Map argument", obj.len());
        }
        _ => {
            log_info!("Parsed single value argument");
        }
    }

    Ok(json.to_string())
}

/// Parse JSON storage into a string for now (will be improved later)
pub fn parse_storage(json: &str) -> Result<String> {
    // Basic validation
    serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON storage: {}", json))?;
    Ok(json.to_string())
}

/// Execute the optimize command
pub fn optimize(args: OptimizeArgs) -> Result<()> {
    println!(
        "Analyzing contract for gas optimization: {:?}",
        args.contract
    );

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    // Load network snapshot if provided
    if let Some(snapshot_path) = &args.network_snapshot {
        println!("\nLoading network snapshot: {:?}", snapshot_path);
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        println!("{}", loaded_snapshot.format_summary());
    }

    let functions_to_analyze = if args.function.is_empty() {
        println!("No functions specified, analyzing all exported functions...");
        crate::utils::wasm::parse_functions(&wasm_bytes)?
    } else {
        args.function.clone()
    };

    let mut executor = ContractExecutor::new(wasm_bytes)?;

    if let Some(storage_json) = &args.storage {
        let storage = parse_storage(storage_json)?;
        executor.set_initial_storage(storage)?;
    }

    let mut optimizer = crate::profiler::analyzer::GasOptimizer::new(executor);

    println!("\nAnalyzing {} function(s)...", functions_to_analyze.len());

    for function_name in &functions_to_analyze {
        println!("  Analyzing function: {}", function_name);
        match optimizer.analyze_function(function_name, args.args.as_deref()) {
            Ok(profile) => {
                println!(
                    "    CPU: {} instructions, Memory: {} bytes",
                    profile.total_cpu, profile.total_memory
                );
            }
            Err(e) => {
                eprintln!(
                    "    Warning: Failed to analyze function {}: {}",
                    function_name, e
                );
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

/// Execute the upgrade-check command
pub fn upgrade_check(args: UpgradeCheckArgs) -> Result<()> {
    println!("Comparing contracts...");
    println!("  Old: {:?}", args.old);
    println!("  New: {:?}", args.new);

    let old_bytes = fs::read(&args.old)
        .with_context(|| format!("Failed to read old WASM file: {:?}", args.old))?;
    let new_bytes = fs::read(&args.new)
        .with_context(|| format!("Failed to read new WASM file: {:?}", args.new))?;

    println!(
        "Loaded contracts (Old: {} bytes, New: {} bytes)",
        old_bytes.len(),
        new_bytes.len()
    );

    let analyzer = crate::analyzer::upgrade::UpgradeAnalyzer::new();

    println!("Running analysis...");
    let report = analyzer.analyze(
        &old_bytes,
        &new_bytes,
        args.function.as_deref(),
        args.args.as_deref(),
    )?;

    let markdown = analyzer.generate_markdown_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &markdown)
            .with_context(|| format!("Failed to write report to: {:?}", output_path))?;
        println!("\nCompatibility report written to: {:?}", output_path);
    } else {
        println!("\n{}", markdown);
    }

    Ok(())
}

/// Execute the compare command
pub fn compare(args: CompareArgs) -> Result<()> {
    println!("Loading trace A: {:?}", args.trace_a);
    let trace_a = crate::compare::ExecutionTrace::from_file(&args.trace_a)?;

    println!("Loading trace B: {:?}", args.trace_b);
    let trace_b = crate::compare::ExecutionTrace::from_file(&args.trace_b)?;

    println!("Comparing traces...\n");
    let report = crate::compare::CompareEngine::compare(&trace_a, &trace_b);
    let rendered = crate::compare::CompareEngine::render_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &rendered)
            .with_context(|| format!("Failed to write report to: {:?}", output_path))?;
        println!("Comparison report written to: {:?}", output_path);
    } else {
        println!("{}", rendered);
    }

    Ok(())
}
