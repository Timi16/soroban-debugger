use crate::cli::args::{InspectArgs, InteractiveArgs, OptimizeArgs, ProfileArgs, RunArgs};
use crate::cli::args::{
    CompareArgs, InspectArgs, InteractiveArgs, OptimizeArgs, RunArgs, UpgradeCheckArgs, Verbosity,
};
use crate::debugger::engine::DebuggerEngine;
use crate::debugger::instruction_pointer::StepMode;
use crate::logging;
use crate::repeat::RepeatRunner;
use crate::runtime::executor::ContractExecutor;
use crate::simulator::SnapshotLoader;
use crate::ui::formatter::Formatter;
use crate::ui::tui::DebuggerUI;
use crate::Result;
use anyhow::Context;
use std::fs;

fn print_info(message: impl AsRef<str>) {
    println!("{}", Formatter::info(message));
}

fn print_success(message: impl AsRef<str>) {
    println!("{}", Formatter::success(message));
}

fn print_warning(message: impl AsRef<str>) {
    println!("{}", Formatter::warning(message));
}

/// Execute the run command.
pub fn run(args: RunArgs, _verbosity: Verbosity) -> Result<()> {
    if args.dry_run {
        return run_dry_run(&args);
    }

    print_info(format!("Loading contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));
    logging::log_contract_loaded(wasm_bytes.len());

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("\nLoading network snapshot: {:?}", snapshot_path));
        logging::log_loading_snapshot(&snapshot_path.to_string_lossy());
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        logging::log_display(loaded_snapshot.format_summary(), logging::LogLevel::Info);
    }

    let parsed_args = if let Some(args_json) = &args.args {
        Some(parse_args(args_json)?)
    } else {
        None
    };

    let initial_storage = if let Some(storage_json) = &args.storage {
        Some(parse_storage(storage_json)?)
    } else {
        None
    };

    if let Some(n) = args.repeat {
        logging::log_repeat_execution(&args.function, n as usize);
        let runner = RepeatRunner::new(wasm_bytes, args.breakpoint, initial_storage);
        let stats = runner.run(&args.function, parsed_args.as_deref(), n)?;
        stats.display();
        return Ok(());
    }

    print_info("\nStarting debugger...");
    print_info(format!("Function: {}", args.function));
    if let Some(ref parsed) = parsed_args {
        print_info(format!("Arguments: {}", parsed));
    }
    logging::log_execution_start(&args.function, parsed_args.as_deref());

    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;
    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint);

    if args.instruction_debug {
        print_info("Enabling instruction-level debugging...");
        engine.enable_instruction_debug(&wasm_bytes)?;

        if args.step_instructions {
            let step_mode = parse_step_mode(&args.step_mode);
            print_info(format!(
                "Starting instruction stepping in '{}' mode",
                args.step_mode
            ));
            engine.start_instruction_stepping(step_mode)?;
            run_instruction_stepping(&mut engine, &args.function, parsed_args.as_deref())?;
            return Ok(());
        }
    }

    print_info("\n--- Execution Start ---\n");
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    print_success("\n--- Execution Complete ---\n");
    print_success(format!("Result: {:?}", result));
    logging::log_execution_complete(&result);

    let mut json_events = None;
    if args.show_events {
        print_info("\n--- Events ---");
        let events = engine.executor().get_events()?;
        let filtered_events = if let Some(topic) = &args.filter_topic {
            crate::inspector::events::EventInspector::filter_events(&events, topic)
        } else {
            events
        };

        if filtered_events.is_empty() {
            print_warning("No events captured.");
        } else {
            for (i, event) in filtered_events.iter().enumerate() {
                print_info(format!("Event #{}:", i));
                if let Some(contract_id) = &event.contract_id {
                    logging::log_event_emitted(contract_id, event.topics.len());
                }
                print_info(format!(
                    "  Contract: {}",
                    event.contract_id.as_deref().unwrap_or("<none>")
                ));
                print_info(format!("  Topics: {:?}", event.topics));
                print_info(format!("  Data: {}", event.data));
            }
        }

        json_events = Some(filtered_events);
    }

    if !args.storage_filter.is_empty() {
        let storage_filter = crate::inspector::storage::StorageFilter::new(&args.storage_filter)
            .map_err(|e| anyhow::anyhow!("Invalid storage filter: {}", e))?;

        print_info("\n--- Storage ---");
        let inspector = crate::inspector::StorageInspector::new();
        inspector.display_filtered(&storage_filter);
        print_info("(Storage view is currently placeholder data)");
    }

    let mut json_auth = None;
    if args.show_auth {
        let auth_tree = engine.executor().get_auth_tree()?;
        if args.json {
            let json_output = crate::inspector::auth::AuthInspector::to_json(&auth_tree)?;
            println!("{}", json_output);
        } else {
            println!("\n--- Authorizations ---");
            crate::inspector::auth::AuthInspector::display(&auth_tree);
        }
        json_auth = Some(auth_tree);
    }

    if args.json
        || args
            .format
            .as_deref()
            .map(|f| f.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
    {
        let mut output = serde_json::json!({
            "result": result,
        });

        if let Some(events) = json_events {
            output["events"] = serde_json::Value::Array(
                events
                    .into_iter()
                    .map(|event| {
                        serde_json::json!({
                            "contract_id": event.contract_id,
                            "topics": event.topics,
                            "data": event.data,
                        })
                    })
                    .collect(),
            );
        }
        if let Some(auth_tree) = json_auth {
            output["auth"] = serde_json::to_value(auth_tree).unwrap_or(serde_json::Value::Null);
        }

        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

/// Execute run command in dry-run mode.
fn run_dry_run(args: &RunArgs) -> Result<()> {
    print_info(format!("[DRY RUN] Loading contract: {:?}", args.contract));

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    print_success(format!(
        "[DRY RUN] Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!(
            "\n[DRY RUN] Loading network snapshot: {:?}",
            snapshot_path
        ));
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        print_info(format!("[DRY RUN] {}", loaded_snapshot.format_summary()));
    }

    let parsed_args = if let Some(args_json) = &args.args {
        Some(parse_args(args_json)?)
    } else {
        None
    };

    let initial_storage = if let Some(storage_json) = &args.storage {
        Some(parse_storage(storage_json)?)
    } else {
        None
    };

    let mut executor = ContractExecutor::new(wasm_bytes)?;
    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    let storage_snapshot = executor.snapshot_storage()?;

    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone());

    print_info("\n[DRY RUN] --- Execution Start ---\n");
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    print_success("\n[DRY RUN] --- Execution Complete ---\n");
    print_success(format!("[DRY RUN] Result: {:?}", result));

    if args.show_events {
        print_info("\n[DRY RUN] --- Events ---");
        let events = engine.executor().get_events()?;
        let filtered_events = if let Some(topic) = &args.filter_topic {
            crate::inspector::events::EventInspector::filter_events(&events, topic)
        } else {
            events
        };

        if filtered_events.is_empty() {
            print_warning("[DRY RUN] No events captured.");
        } else {
            for (i, event) in filtered_events.iter().enumerate() {
                print_info(format!("[DRY RUN] Event #{}:", i));
                print_info(format!(
                    "[DRY RUN]   Contract: {}",
                    event.contract_id.as_deref().unwrap_or("<none>")
                ));
                print_info(format!("[DRY RUN]   Topics: {:?}", event.topics));
                print_info(format!("[DRY RUN]   Data: {}", event.data));
            }
        }
    }

    engine.executor_mut().restore_storage(&storage_snapshot)?;
    print_success("\n[DRY RUN] Storage state restored (changes rolled back)");

    Ok(())
}

/// Execute the interactive command.
pub fn interactive(args: InteractiveArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!(
        "Starting interactive debugger for: {:?}",
        args.contract
    ));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));
    logging::log_contract_loaded(wasm_bytes.len());

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("\nLoading network snapshot: {:?}", snapshot_path));
        logging::log_loading_snapshot(&snapshot_path.to_string_lossy());
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        logging::log_display(loaded_snapshot.format_summary(), logging::LogLevel::Info);
    }

    let executor = ContractExecutor::new(wasm_bytes)?;
    let engine = DebuggerEngine::new(executor, vec![]);

    print_info("\nStarting interactive mode...");
    print_info("Type 'help' for available commands\n");
    logging::log_interactive_mode_start();

    let mut ui = DebuggerUI::new(engine)?;
    ui.run()?;

    Ok(())
}

/// Execute the inspect command.
pub fn inspect(args: InspectArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Inspecting contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    let module_info = crate::utils::wasm::get_module_info(&wasm_bytes)?;

    println!("\n{}", "=".repeat(54));
    println!("  Soroban Contract Inspector");
    println!("{}", "=".repeat(54));
    println!("\n  File : {:?}", args.contract);
    println!("  Size : {} bytes", wasm_bytes.len());

    println!("\n{}", "-".repeat(54));
    println!("  Module Information");
    println!("{}", "-".repeat(54));
    println!("  Types      : {}", module_info.type_count);
    println!("  Functions  : {}", module_info.function_count);
    println!("  Exports    : {}", module_info.export_count);

    if args.functions {
        println!("\n{}", "-".repeat(54));
        println!("  Exported Functions");
        println!("{}", "-".repeat(54));

        let functions = crate::utils::wasm::parse_functions(&wasm_bytes)?;
        if functions.is_empty() {
            println!("  (No exported functions found)");
        } else {
            for func in functions {
                println!("  - {}", func);
            }
        }
    }

    if args.metadata {
        println!("\n{}", "-".repeat(54));
        println!("  Contract Metadata");
        println!("{}", "-".repeat(54));

        let metadata = crate::utils::wasm::extract_contract_metadata(&wasm_bytes)?;
        if metadata.is_empty() {
            println!("  (No embedded metadata found)");
        } else {
            if let Some(version) = metadata.contract_version {
                println!("  Contract version      : {}", version);
            }
            if let Some(sdk) = metadata.sdk_version {
                println!("  Soroban SDK version   : {}", sdk);
            }
            if let Some(build_date) = metadata.build_date {
                println!("  Build date            : {}", build_date);
            }
            if let Some(author) = metadata.author {
                println!("  Author / organization : {}", author);
            }
            if let Some(desc) = metadata.description {
                println!("  Description           : {}", desc);
            }
            if let Some(impl_notes) = metadata.implementation {
                println!("  Implementation notes  : {}", impl_notes);
            }
        }
    }

    println!("\n{}", "=".repeat(54));
    Ok(())
}

/// Execute the optimize command
pub fn optimize(args: OptimizeArgs) -> Result<()> {
    println!(
/// Parse JSON arguments with validation.
pub fn parse_args(json: &str) -> Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON arguments: {}", json))?;

    match value {
        serde_json::Value::Array(ref arr) => {
            tracing::debug!(count = arr.len(), "Parsed array arguments");
        }
        serde_json::Value::Object(ref obj) => {
            tracing::debug!(fields = obj.len(), "Parsed object arguments");
        }
        _ => {
            tracing::debug!("Parsed single value argument");
        }
    }

    Ok(json.to_string())
}

/// Parse JSON storage.
pub fn parse_storage(json: &str) -> Result<String> {
    serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON storage: {}", json))?;
    Ok(json.to_string())
}

/// Execute the optimize command.
pub fn optimize(args: OptimizeArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!(
        "Analyzing contract for gas optimization: {:?}",
        args.contract
    ));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));
    logging::log_contract_loaded(wasm_bytes.len());

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("\nLoading network snapshot: {:?}", snapshot_path));
        logging::log_loading_snapshot(&snapshot_path.to_string_lossy());
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        logging::log_display(loaded_snapshot.format_summary(), logging::LogLevel::Info);
    }

    let functions_to_analyze = if args.function.is_empty() {
        print_warning("No functions specified, analyzing all exported functions...");
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

    print_info(format!(
        "\nAnalyzing {} function(s)...",
        functions_to_analyze.len()
    ));
    logging::log_analysis_start("gas optimization");

    for function_name in &functions_to_analyze {
        print_info(format!("  Analyzing function: {}", function_name));
        match optimizer.analyze_function(function_name, args.args.as_deref()) {
            Ok(profile) => {
                println!(
                    "    CPU: {} instructions, Memory: {} bytes, Time: {} ms",
                    profile.total_cpu, profile.total_memory, profile.wall_time_ms
                );
                print_success(format!(
                    "    CPU: {} instructions, Memory: {} bytes",
                    profile.total_cpu, profile.total_memory
                ));
            }
            Err(e) => {
                print_warning(format!(
                    "    Warning: Failed to analyze function {}: {}",
                    function_name, e
                ));
                tracing::warn!(function = function_name, error = %e, "Failed to analyze function");
            }
        }
    }
    logging::log_analysis_complete("gas optimization", functions_to_analyze.len());

    let contract_path_str = args.contract.to_string_lossy().to_string();
    let report = optimizer.generate_report(&contract_path_str);
    let markdown = optimizer.generate_markdown_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &markdown)
            .with_context(|| format!("Failed to write report to: {:?}", output_path))?;
        print_success(format!(
            "\nOptimization report written to: {:?}",
            output_path
        ));
        logging::log_optimization_report(&output_path.to_string_lossy());
    } else {
        logging::log_display(&markdown, logging::LogLevel::Info);
    }

    Ok(())
}

/// ✅ Execute the profile command (hotspots + suggestions)
pub fn profile(args: ProfileArgs) -> Result<()> {
    println!("Profiling contract execution: {:?}", args.contract);

    // Load WASM file
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!("Contract loaded successfully ({} bytes)", wasm_bytes.len());

    // Parse args (optional)
    let parsed_args = if let Some(args_json) = &args.args {
        Some(parse_args(args_json)?)
    } else {
        None
    };

    // Create executor
    let mut executor = ContractExecutor::new(wasm_bytes)?;

    // Initial storage (optional)
    if let Some(storage_json) = &args.storage {
        let storage = parse_storage(storage_json)?;
        executor.set_initial_storage(storage)?;
    }

    // Analyze exactly one function (this command focuses on execution hotspots)
    let mut optimizer = crate::profiler::analyzer::GasOptimizer::new(executor);

    println!("\nRunning function: {}", args.function);
    if let Some(ref a) = parsed_args {
        println!("Args: {}", a);
    }

    let _profile = optimizer.analyze_function(&args.function, parsed_args.as_deref())?;

    let contract_path_str = args.contract.to_string_lossy().to_string();
    let report = optimizer.generate_report(&contract_path_str);

    // Hotspot summary first
    println!("\n{}", report.format_hotspots());

    // Then detailed suggestions (markdown format)
    let markdown = optimizer.generate_markdown_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &markdown)
            .with_context(|| format!("Failed to write report to: {:?}", output_path))?;
        println!("\nProfile report written to: {:?}", output_path);
    } else {
        println!("\n{}", markdown);
/// Execute the upgrade-check command.
pub fn upgrade_check(args: UpgradeCheckArgs, _verbosity: Verbosity) -> Result<()> {
    print_info("Comparing contracts...");
    print_info(format!("  Old: {:?}", args.old));
    print_info(format!("  New: {:?}", args.new));
    logging::log_contract_comparison(&args.old.to_string_lossy(), &args.new.to_string_lossy());

    let old_bytes = fs::read(&args.old)
        .with_context(|| format!("Failed to read old WASM file: {:?}", args.old))?;
    let new_bytes = fs::read(&args.new)
        .with_context(|| format!("Failed to read new WASM file: {:?}", args.new))?;

    print_success(format!(
        "Loaded contracts (Old: {} bytes, New: {} bytes)",
        old_bytes.len(),
        new_bytes.len()
    ));

    print_info("Running analysis...");
    tracing::info!(
        old_size = old_bytes.len(),
        new_size = new_bytes.len(),
        "Loaded contracts for comparison"
    );

    let analyzer = crate::analyzer::upgrade::UpgradeAnalyzer::new();
    logging::log_analysis_start("contract upgrade compatibility check");
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
        print_success(format!(
            "\nCompatibility report written to: {:?}",
            output_path
        ));
        logging::log_optimization_report(&output_path.to_string_lossy());
    } else {
        logging::log_display(&markdown, logging::LogLevel::Info);
    }

    Ok(())
}

/// Execute the compare command.
pub fn compare(args: CompareArgs) -> Result<()> {
    print_info(format!("Loading trace A: {:?}", args.trace_a));
    let trace_a = crate::compare::ExecutionTrace::from_file(&args.trace_a)?;

    print_info(format!("Loading trace B: {:?}", args.trace_b));
    let trace_b = crate::compare::ExecutionTrace::from_file(&args.trace_b)?;

    print_info("Comparing traces...");
    let report = crate::compare::CompareEngine::compare(&trace_a, &trace_b);
    let rendered = crate::compare::CompareEngine::render_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &rendered)
            .with_context(|| format!("Failed to write report to: {:?}", output_path))?;
        print_success(format!("Comparison report written to: {:?}", output_path));
    } else {
        println!("{}", rendered);
    }

    Ok(())
}

/// Run instruction-level stepping mode.
fn run_instruction_stepping(
    engine: &mut DebuggerEngine,
    function: &str,
    args: Option<&str>,
) -> Result<()> {
    println!("\n=== Instruction Stepping Mode ===");
    println!("Type 'help' for available commands\n");

    display_instruction_context(engine, 3);

    loop {
        print!("(step) > ");
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "n" | "next" | "s" | "step" | "into" | "" => match engine.step_into() {
                Ok(true) => {
                    println!("Stepped to next instruction");
                    display_instruction_context(engine, 3);
                }
                Ok(false) => println!("Cannot step: execution finished or error occurred"),
                Err(e) => println!("Error stepping: {}", e),
            },
            "o" | "over" => match engine.step_over() {
                Ok(true) => {
                    println!("Stepped over instruction");
                    display_instruction_context(engine, 3);
                }
                Ok(false) => println!("Cannot step over: execution finished or error occurred"),
                Err(e) => println!("Error stepping: {}", e),
            },
            "u" | "out" => match engine.step_out() {
                Ok(true) => {
                    println!("Stepped out of function");
                    display_instruction_context(engine, 3);
                }
                Ok(false) => println!("Cannot step out: execution finished or error occurred"),
                Err(e) => println!("Error stepping: {}", e),
            },
            "b" | "block" => match engine.step_block() {
                Ok(true) => {
                    println!("Stepped to next basic block");
                    display_instruction_context(engine, 3);
                }
                Ok(false) => {
                    println!("Cannot step to next block: execution finished or error occurred")
                }
                Err(e) => println!("Error stepping: {}", e),
            },
            "p" | "prev" | "back" => match engine.step_back() {
                Ok(true) => {
                    println!("Stepped back to previous instruction");
                    display_instruction_context(engine, 3);
                }
                Ok(false) => println!("Cannot step back: no previous instruction"),
                Err(e) => println!("Error stepping: {}", e),
            },
            "c" | "continue" => {
                println!("Continuing execution...");
                engine.continue_execution()?;
                let result = engine.execute(function, args)?;
                println!("Execution completed. Result: {:?}", result);
                break;
            }
            "i" | "info" => display_instruction_info(engine),
            "ctx" | "context" => {
                print!("Enter context size (default 5): ");
                std::io::Write::flush(&mut std::io::stdout())?;
                let mut size_input = String::new();
                std::io::stdin().read_line(&mut size_input)?;
                let size = size_input.trim().parse().unwrap_or(5);
                display_instruction_context(engine, size);
            }
            "h" | "help" => println!("{}", Formatter::format_stepping_help()),
            "q" | "quit" | "exit" => {
                println!("Exiting instruction stepping mode...");
                break;
            }
            _ => {
                println!(
                    "Unknown command: {}. Type 'help' for available commands.",
                    input
                );
            }
        }
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
fn display_instruction_context(engine: &DebuggerEngine, context_size: usize) {
    let context = engine.get_instruction_context(context_size);
    let formatted = Formatter::format_instruction_context(&context, context_size);
    println!("{}", formatted);
}

fn display_instruction_info(engine: &DebuggerEngine) {
    if let Ok(state) = engine.state().lock() {
        let ip = state.instruction_pointer();
        let step_mode = if ip.is_stepping() {
            Some(ip.step_mode())
        } else {
            None
        };

        println!(
            "{}",
            Formatter::format_instruction_pointer_state(
                ip.current_index(),
                ip.call_stack_depth(),
                step_mode,
                ip.is_stepping(),
            )
        );

        println!(
            "{}",
            Formatter::format_instruction_stats(
                state.instructions().len(),
                ip.current_index(),
                state.step_count(),
            )
        );

        if let Some(current_inst) = state.current_instruction() {
            println!("Current Instruction Details:");
            println!("  Name: {}", current_inst.name());
            println!("  Offset: 0x{:08x}", current_inst.offset);
            println!("  Function: {}", current_inst.function_index);
            println!("  Local Index: {}", current_inst.local_index);
            println!("  Operands: {}", current_inst.operands());
            println!("  Control Flow: {}", current_inst.is_control_flow());
            println!("  Function Call: {}", current_inst.is_call());
        }
    } else {
        println!("Cannot access debug state");
    }
}

fn parse_step_mode(step_mode: &str) -> StepMode {
    match step_mode.to_lowercase().as_str() {
        "into" => StepMode::StepInto,
        "over" => StepMode::StepOver,
        "out" => StepMode::StepOut,
        "block" => StepMode::StepBlock,
        _ => StepMode::StepInto,
    }
}
