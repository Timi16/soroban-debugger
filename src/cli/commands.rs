use crate::cli::args::{
    CompareArgs, InspectArgs, InteractiveArgs, OptimizeArgs, RunArgs, UpgradeCheckArgs, Verbosity,
    CompareArgs, InspectArgs, InteractiveArgs, OptimizeArgs, ProfileArgs, RunArgs, SymbolicArgs,
    TuiArgs, UpgradeCheckArgs, Verbosity,
};
use crate::debugger::engine::DebuggerEngine;
use crate::debugger::instruction_pointer::StepMode;
use crate::logging;
use crate::output::OutputConfig;
use crate::repeat::RepeatRunner;
use crate::runtime::executor::ContractExecutor;
use crate::simulator::SnapshotLoader;
use crate::ui::formatter::Formatter;
use crate::ui::tui::DebuggerUI;
use crate::Result;
use anyhow::Context;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::fs;
use std::sync::mpsc;
use std::time::Duration;
use textplots::{Chart, Plot, Shape};

fn print_info(message: impl AsRef<str>) {
    println!("{}", Formatter::info(message));
}

fn print_success(message: impl AsRef<str>) {
    println!("{}", Formatter::success(message));
}

fn print_warning(message: impl AsRef<str>) {
    println!("{}", Formatter::warning(message));
}

/// Execute the run command
pub fn run(args: RunArgs, _verbosity: Verbosity) -> Result<()> {
/// Execute watch mode: monitor WASM file and re-run on changes
fn run_watch_mode(args: RunArgs) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    print_info(format!("Starting watch mode for: {:?}", args.contract));
    print_info("Press Ctrl+C to exit");

    // Canonicalize the path to ensure we're watching the right file
    let watch_path = args
        .contract
        .canonicalize()
        .with_context(|| format!("Failed to resolve contract path: {:?}", args.contract))?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    // Create a channel for file system events
    let (tx, rx) = mpsc::channel();

    // Create a debouncer to avoid repeated triggers
    let mut last_run = std::time::Instant::now();
    let debounce_duration = Duration::from_millis(500);

    // Set up file watcher
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            // Only trigger on modify events
            if matches!(event.kind, EventKind::Modify(_)) {
                let _ = tx.send(());
            }
        }
    })
    .context("Failed to create file watcher")?;

    // Watch the parent directory (watching a file directly doesn't work reliably)
    let watch_dir = watch_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Contract file has no parent directory"))?;

    watcher
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("Failed to watch directory: {:?}", watch_dir))?;

    // Run once immediately
    print_info("\n--- Initial Run ---\n");
    if let Err(e) = run_once(&args) {
        print_warning(format!("Execution failed: {}", e));
    }

    // Watch loop
    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(()) => {
                // Debounce: only run if enough time has passed
                let now = std::time::Instant::now();
                if now.duration_since(last_run) < debounce_duration {
                    continue;
                }
                last_run = now;

                // Check if the specific file we're watching was modified
                if !watch_path.exists() {
                    print_warning("Contract file no longer exists, waiting...");
                    continue;
                }

                // Clear terminal for clean output
                clear_terminal();

                print_info(format!("File changed: {:?}", args.contract));
                print_info("Re-running...\n");

                // Re-run the contract
                if let Err(e) = run_once(&args) {
                    print_warning(format!("Execution failed: {}", e));
                    print_info("\nWaiting for changes...");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Normal timeout, continue loop
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Watcher disconnected, exit
                break;
            }
        }
    }

    print_info("\nWatch mode exited cleanly");
    Ok(())
}

/// Execute a single run of the contract (used by watch mode)
fn run_once(args: &RunArgs) -> Result<()> {
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    if let Some(snapshot_path) = &args.network_snapshot {
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        loader.apply_to_environment()?;
    }

    let parsed_args = if let Some(args_json) = &args.args {
        Some(parse_args(args_json)?)
    } else {
        None
    };

    let mut initial_storage = if let Some(storage_json) = &args.storage {
        Some(parse_storage(storage_json)?)
    } else {
        None
    };

    if let Some(import_path) = &args.import_storage {
        let imported = crate::inspector::storage::StorageState::import_from_file(import_path)?;
        initial_storage = Some(serde_json::to_string(&imported)?);
    }

    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;
    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }
    if !args.mock.is_empty() {
        executor.set_mock_specs(&args.mock)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone());

    print_info("--- Execution Start ---\n");
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    print_success("\n--- Execution Complete ---");
    print_success(format!("Result: {:?}", result));

    if args.show_events {
        let events = engine.executor().get_events()?;
        let filtered_events = if let Some(topic) = &args.filter_topic {
            crate::inspector::events::EventInspector::filter_events(&events, topic)
        } else {
            events
        };

        if !filtered_events.is_empty() {
            print_info("\n--- Events ---");
            for (i, event) in filtered_events.iter().enumerate() {
                print_info(format!("Event #{}: {:?}", i, event));
            }
        }
    }

    if args.show_auth {
        let auth_tree = engine.executor().get_auth_tree()?;
        println!("\n--- Authorizations ---");
        crate::inspector::auth::AuthInspector::display(&auth_tree);
    }

    print_info("\nWaiting for changes...");
    Ok(())
}

/// Clear the terminal screen
fn clear_terminal() {
    print!("\x1B[2J\x1B[1;1H");
    std::io::Write::flush(&mut std::io::stdout()).ok();
}

/// Execute batch mode with parallel execution
fn run_batch(args: &RunArgs, batch_file: &std::path::Path) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));
    logging::log_contract_loaded(wasm_bytes.len());

    print_info(format!("Loading batch file: {:?}", batch_file));
    let batch_items = crate::batch::BatchExecutor::load_batch_file(batch_file)?;
    print_success(format!("Loaded {} test cases", batch_items.len()));

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("\nLoading network snapshot: {:?}", snapshot_path));
        logging::log_loading_snapshot(&snapshot_path.to_string_lossy());
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        logging::log_display(loaded_snapshot.format_summary(), logging::LogLevel::Info);
    }

    print_info(format!(
        "\nExecuting {} test cases in parallel for function: {}",
        batch_items.len(),
        args.function
    ));
    logging::log_execution_start(&args.function, None);

    let executor = crate::batch::BatchExecutor::new(wasm_bytes, args.function.clone());
    let results = executor.execute_batch(batch_items)?;
    let summary = crate::batch::BatchExecutor::summarize(&results);

    crate::batch::BatchExecutor::display_results(&results, &summary);

    if args.json
        || args
            .format
            .as_deref()
            .map(|f| f.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
    {
        let output = serde_json::json!({
            "results": results,
            "summary": summary,
        });
        println!("\n{}", serde_json::to_string_pretty(&output)?);
    }

    logging::log_execution_complete(&format!("{}/{} passed", summary.passed, summary.total));

    if summary.failed > 0 || summary.errors > 0 {
        anyhow::bail!(
            "Batch execution completed with failures: {} failed, {} errors",
            summary.failed,
            summary.errors
        );
    }

    Ok(())
}

/// Execute the run command.
pub fn run(args: RunArgs, verbosity: Verbosity) -> Result<()> {
    // Handle batch execution mode
    if let Some(batch_file) = &args.batch_args {
        return run_batch(&args, batch_file);
    }

    // Handle watch mode
    if args.watch {
        return run_watch_mode(args);
    }

    if args.dry_run {
        return run_dry_run(&args);
    }

    print_info(format!("Loading contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    let checksum = crate::utils::wasm::compute_checksum(&wasm_bytes);
    if verbosity == Verbosity::Verbose {
        print_info(format!("WASM SHA-256 Checksum: {}", checksum));
    }
    if let Some(ref expected) = args.expected_hash {
        if checksum != *expected {
            anyhow::bail!(
                "WASM checksum mismatch! Expected {}, but got {}",
                expected,
                checksum
            );
        }
    }

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
        let runner = RepeatRunner::new(wasm_bytes, args.breakpoint, args.condition, initial_storage);
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

    // Create executor
    let mut executor = ContractExecutor::new(wasm_bytes)?;
    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;
    executor.set_timeout(args.timeout);

    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint, args.condition);

    // Enable instruction-level debugging if requested
    if args.instruction_debug {
        println!("Enabling instruction-level debugging...");
        
        // Parse step mode
        let step_mode = match args.step_mode.to_lowercase().as_str() {
            "into" => StepMode::StepInto,
            "over" => StepMode::StepOver,
            "out" => StepMode::StepOut,
            "block" => StepMode::StepBlock,
            _ => {
                println!("Warning: Invalid step mode '{}', using 'into'", args.step_mode);
                StepMode::StepInto
            }
        };

        // Start instruction stepping if requested
        if args.step_instructions {
            println!("Starting instruction stepping in {} mode", args.step_mode);
            engine.start_instruction_stepping(step_mode)?;
            
            // Enter instruction stepping mode
            run_instruction_stepping(&mut engine, &args.function, parsed_args.as_deref())?;
            return Ok(());
        }
    }

    // Execute with debugging
    println!("\n--- Execution Start ---\n");
    print_info("\n--- Execution Start ---\n");
    let storage_before = engine.executor().get_storage_snapshot()?;
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    let storage_after = engine.executor().get_storage_snapshot()?;
    print_success("\n--- Execution Complete ---\n");
    print_success(format!("Result: {:?}", result));
    logging::log_execution_complete(&result);

    let storage_diff = crate::inspector::storage::StorageInspector::compute_diff(
        &storage_before,
        &storage_after,
        &args.alert_on_change,
    );
    if !storage_diff.is_empty() || !args.alert_on_change.is_empty() {
        print_info("\n--- Storage Changes ---");
        crate::inspector::storage::StorageInspector::display_diff(&storage_diff);
    }

    if let Some(export_path) = &args.export_storage {
        print_info(format!("\nExporting storage to: {:?}", export_path));
        crate::inspector::storage::StorageState::export_to_file(&storage_after, export_path)?;
    }
    let mock_calls = engine.executor().get_mock_call_log();
    if !args.mock.is_empty() {
        display_mock_call_log(&mock_calls);
    }

    // Save budget info to history
    let host = engine.executor().host();
    let budget = crate::inspector::budget::BudgetInspector::get_cpu_usage(host);
    if let Ok(manager) = HistoryManager::new() {
        let record = RunHistory {
            date: chrono::Utc::now().to_rfc3339(),
            contract_hash: args.contract.to_string_lossy().to_string(),
            function: args.function.clone(),
            cpu_used: budget.cpu_instructions,
            memory_used: budget.memory_bytes,
        };
        let _ = manager.append_record(record);
    }

    // Export storage if specified
    if let Some(export_path) = &args.export_storage {
        print_info(format!("Exporting storage to: {:?}", export_path));
        let storage_snapshot = engine.executor().get_storage_snapshot()?;
        crate::inspector::storage::StorageState::export_to_file(&storage_snapshot, export_path)?;
        print_success(format!(
            "Exported {} storage entries",
            storage_snapshot.len()
        ));
    }

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
    }

    if !args.storage_filter.is_empty() {
        let storage_filter = crate::inspector::storage::StorageFilter::new(&args.storage_filter)
            .map_err(|e| anyhow::anyhow!("Invalid storage filter: {}", e))?;

        let storage_data = engine
            .executor()
            .get_storage()
            .map_err(|e| anyhow::anyhow!("Failed to get storage data: {}", e))?;

        let inspector = crate::inspector::storage::StorageInspector::new(&storage_data);
        print_info("\n--- Storage ---");
        tracing::info!("Displaying filtered storage");
        
        print_info("\n--- Storage ---");
        tracing::info!("Displaying filtered storage");
        let inspector = crate::inspector::storage::StorageInspector::new();
        inspector.display_filtered(&storage_filter);
    }

    if let Some(format) = &args.format {
        if format.eq_ignore_ascii_case("json") {
            let mut output = serde_json::json!({
                "result": format!("{:?}", result),
            });

            if args.show_events {
                let events = engine.executor().get_events()?;
                output["events"] =
                    serde_json::to_value(&events).unwrap_or(serde_json::Value::Null);
                let event_values: Vec<serde_json::Value> = events
                    .iter()
                    .map(|e| serde_json::json!({
                        "contract_id": e.contract_id.as_deref().unwrap_or("<none>"),
                        "topics": e.topics,
                        "data": e.data
                    }))
                    .collect();
                output["events"] = serde_json::Value::Array(event_values);
            }

            println!("{}", serde_json::to_string_pretty(&output).unwrap());
            return Ok(());
        }
    }

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

    // Export execution trace if requested
    if let Some(trace_path) = &args.trace_output {
        if let Some(trace) = engine.last_trace() {
            let trace_json = trace.to_json()?;
            fs::write(trace_path, trace_json)
                .with_context(|| format!("Failed to write trace to: {:?}", trace_path))?;
            println!("\nExecution trace exported to: {:?}", trace_path);
        }
    }

    // If output format is JSON, print full result as JSON and exit
    if let Some(format) = &args.format {
        if format.eq_ignore_ascii_case("json") {
            let mut output = serde_json::json!({
                "result": format!("{:?}", result),
            });
    if args.json
        || args
            .format
            .as_deref()
            .map(|f| f.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
    {
        let mut output = serde_json::json!({
            "result": result,
            "wasm_hash": checksum,
            "alerts": storage_diff.triggered_alerts,
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
        if !mock_calls.is_empty() {
            output["mock_calls"] = serde_json::Value::Array(
                mock_calls
                    .iter()
                    .map(|entry| {
                        serde_json::json!({
                            "contract_id": entry.contract_id,
                            "function": entry.function,
                            "args_count": entry.args_count,
                            "mocked": entry.mocked,
                            "returned": entry.returned,
                        })
                    })
                    .collect(),
            );
        }

        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

/// Execute the run command in dry-run mode
fn run_dry_run(args: &RunArgs) -> Result<()> {
    println!("[DRY RUN] Loading contract: {:?}", args.contract);

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    println!(
        "[DRY RUN] Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    );

    if let Some(snapshot_path) = &args.network_snapshot {
        println!("\n[DRY RUN] Loading network snapshot: {:?}", snapshot_path);
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        println!("[DRY RUN] {}", loaded_snapshot.format_summary());
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

    println!("\n[DRY RUN] Starting debugger...");
    println!("[DRY RUN] Function: {}", args.function);
    if let Some(ref parsed) = parsed_args {
        println!("[DRY RUN] Arguments: {}", parsed);
    }

    let mut executor = ContractExecutor::new(wasm_bytes)?;

    if let Some(storage) = &initial_storage {
        executor.set_initial_storage(storage.clone())?;
    }

    let storage_snapshot = executor.snapshot_storage()?;
    println!("[DRY RUN] Storage state snapshotted");

    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone());
    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone(), args.condition.clone());

    println!("\n[DRY RUN] --- Execution Start ---\n");
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    println!("\n[DRY RUN] --- Execution Complete ---\n");

    println!("[DRY RUN] Result: {:?}", result);

    if args.show_events {
        println!("\n[DRY RUN] --- Events ---");
        let events = engine.executor().get_events()?;
        let filtered_events = if let Some(topic) = &args.filter_topic {
            crate::inspector::events::EventInspector::filter_events(&events, topic)
        } else {
            events
        };

        if filtered_events.is_empty() {
            println!("[DRY RUN] No events captured.");
        } else {
            for (i, event) in filtered_events.iter().enumerate() {
                println!("[DRY RUN] Event #{}:", i);
                if let Some(contract_id) = &event.contract_id {
                    println!("[DRY RUN]   Contract: {}", contract_id);
                }
                println!("[DRY RUN]   Topics: {:?}", event.topics);
                println!("[DRY RUN]   Data: {}", event.data);
                println!();
            }
        }
    }

    if !args.storage_filter.is_empty() {
        let _storage_filter =
            crate::inspector::storage::StorageFilter::new(&args.storage_filter)
                .map_err(|e| anyhow::anyhow!("Invalid storage filter: {}", e))?;
        println!("\n[DRY RUN] --- Storage (Post-Execution) ---");
        println!("[DRY RUN] Storage changes would be displayed here");
        println!("[DRY RUN] (Storage inspection not yet fully implemented)");
    } else {
        println!("\n[DRY RUN] --- Storage Changes ---");
        println!("[DRY RUN] (Use --storage-filter to view specific storage entries)");
    }

    engine.executor_mut().restore_storage(&storage_snapshot)?;
    println!("\n[DRY RUN] Storage state restored (all changes rolled back)");
    println!("[DRY RUN] Dry-run completed - no persistent changes were made");

    Ok(())
}

/// Execute the interactive command
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
    let engine = DebuggerEngine::new(executor, vec![], vec![]);

    print_info("\nStarting interactive mode...");
    print_info("Type 'help' for available commands\n");
    logging::log_interactive_mode_start();

    let mut ui = DebuggerUI::new(engine)?;
    ui.run()?;

    Ok(())
}

/// Execute the inspect command
/// Launch the full-screen TUI dashboard.
pub fn tui(args: TuiArgs, _verbosity: Verbosity) -> Result<()> {
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    if let Some(snapshot_path) = &args.network_snapshot {
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        loader.apply_to_environment()?;
    }

    let parsed_args = if let Some(ref a) = args.args {
        Some(parse_args(a)?)
    } else {
        None
    };

    let initial_storage = if let Some(ref s) = args.storage {
        Some(parse_storage(s)?)
    } else {
        None
    };

    let mut executor = ContractExecutor::new(wasm_bytes)?;
    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint, args.condition);

    // Pre-execute so live data is available immediately in the dashboard
    let _ = engine.execute(&args.function, parsed_args.as_deref());

    crate::ui::run_dashboard(engine, &args.function)?;

    Ok(())
}

/// Execute the inspect command.
pub fn inspect(args: InspectArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Inspecting contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    let module_info = crate::utils::wasm::get_module_info(&wasm_bytes)?;

    println!("\n{}", OutputConfig::double_rule_line(54));
    println!("  Soroban Contract Inspector");
    println!("  {}", OutputConfig::double_rule_line(54));
    println!("\n  File : {:?}", args.contract);
    println!("  Size : {} bytes", wasm_bytes.len());

    println!("\n{}", OutputConfig::rule_line(54));
    println!("  Module Information");
    println!("  {}", OutputConfig::rule_line(52));
    println!("  Types      : {}", module_info.type_count);
    println!("  Functions  : {}", module_info.function_count);
    println!("  Exports    : {}", module_info.export_count);

    if args.functions {
        println!("\n{}", OutputConfig::rule_line(54));
        println!("  Exported Functions");
        println!("  {}", OutputConfig::rule_line(52));

        let functions = crate::utils::wasm::parse_functions(&wasm_bytes)?;
        if functions.is_empty() {
            println!("  (No exported functions found)");
        } else {
            for function in functions {
                println!("  {} {}", OutputConfig::to_ascii("•"), function);
            }
        }
    }

    if args.dependency_graph {
        println!("\n{}", OutputConfig::rule_line(54));
        println!("  Contract Dependency Graph");
        println!("  {}", OutputConfig::rule_line(52));

        let calls = crate::utils::wasm::parse_cross_contract_calls(&wasm_bytes)?;
        if calls.is_empty() {
            println!("  (No cross-contract call instructions detected)");
        } else {
            let contract_name = args
                .contract
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("contract")
                .to_string();

            let mut graph = crate::analyzer::graph::DependencyGraph::new();
            graph.add_node(contract_name.clone());
            for call in calls {
                graph.add_edge(contract_name.clone(), call.target);
            }

            println!("\nDOT:");
            println!("{}", graph.to_dot());
            println!("\nMermaid:");
            println!("{}", graph.to_mermaid());
        }
    }

    if args.metadata {
        println!("\n{}", OutputConfig::rule_line(54));
        println!("  Contract Metadata");
        println!("  {}", OutputConfig::rule_line(52));

        match crate::utils::wasm::extract_contract_metadata(&wasm_bytes) {
            Ok(metadata) => {
                if metadata.is_empty() {
                    println!(
                        "  {}  No metadata section embedded in this contract",
                        OutputConfig::to_ascii("⚠")
                    );
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
            Err(e) => {
                println!("  Error reading metadata: {}", e);
                println!("  (This may indicate a corrupted metadata section)");
            }
        }
    }

    println!("\n{}", OutputConfig::double_rule_line(54));
    Ok(())
}

/// Parse JSON arguments with validation
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

/// Parse JSON storage into a string
pub fn parse_storage(json: &str) -> Result<String> {
    serde_json::from_str::<serde_json::Value>(json)
        .with_context(|| format!("Invalid JSON storage: {}", json))?;
    Ok(json.to_string())
}

/// Execute the optimize command
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

/// Execute the upgrade-check command
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

/// Run instruction-level stepping mode
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

/// Execute the symbolic command.
pub fn symbolic(args: SymbolicArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!(
        "Starting symbolic execution analysis for contract: {:?}",
        args.contract
    ));
    let wasm_bytes = fs::read(&args.contract)
        .with_context(|| format!("Failed to read WASM file {:?}", args.contract))?;

    let analyzer = crate::analyzer::symbolic::SymbolicAnalyzer::new();
    let report = analyzer.analyze(&wasm_bytes, &args.function)?;

    print_success(format!("Paths explored: {}", report.paths_explored));
    print_success(format!("Panics found: {}", report.panics_found));

    let toml = analyzer.generate_scenario_toml(&report);
    if let Some(out) = args.output {
        fs::write(&out, toml).context("Failed to write toml")?;
        print_success(format!("Wrote scenario to {:?}", out));
    } else {
        println!("{}", toml);
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

    // Display initial instruction context
    display_instruction_context(engine, 3);

    loop {
        print!("(step) > ");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "n" | "next" => {
                if let Ok(stepped) = engine.step_into() {
                    if stepped {
                        println!("Stepped to next instruction");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step: execution finished or error occurred");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            "s" | "step" | "into" => {
                if let Ok(stepped) = engine.step_into() {
                    if stepped {
                        println!("Stepped into next instruction");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step into: execution finished or error occurred");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            "o" | "over" => {
                if let Ok(stepped) = engine.step_over() {
                    if stepped {
                        println!("Stepped over instruction");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step over: execution finished or error occurred");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            "u" | "out" => {
                if let Ok(stepped) = engine.step_out() {
                    if stepped {
                        println!("Stepped out of function");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step out: execution finished or error occurred");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            "b" | "block" => {
                if let Ok(stepped) = engine.step_block() {
                    if stepped {
                        println!("Stepped to next basic block");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step to next block: execution finished or error occurred");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            "p" | "prev" | "back" => {
                if let Ok(stepped) = engine.step_back() {
                    if stepped {
                        println!("Stepped back to previous instruction");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step back: no previous instruction");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            "c" | "continue" => {
                println!("Continuing execution...");
                engine.continue_execution()?;
                
                // Execute the function
                let result = engine.execute(function, args)?;
                println!("Execution completed. Result: {:?}", result);
                break;
            }
            "i" | "info" => {
                display_instruction_info(engine);
            }
            "ctx" | "context" => {
                print!("Enter context size (default 5): ");
                std::io::Write::flush(&mut std::io::stdout()).unwrap();
                let mut size_input = String::new();
                std::io::stdin().read_line(&mut size_input).unwrap();
                let size = size_input.trim().parse().unwrap_or(5);
                display_instruction_context(engine, size);
            }
            "h" | "help" => {
                println!("{}", Formatter::format_stepping_help());
            }
            "q" | "quit" | "exit" => {
                println!("Exiting instruction stepping mode...");
                break;
            }
            "" => {
                // Repeat last command (step into)
                if let Ok(stepped) = engine.step_into() {
                    if stepped {
                        println!("Stepped to next instruction");
                        display_instruction_context(engine, 3);
                    } else {
                        println!("Cannot step: execution finished or error occurred");
                    }
                } else {
                    println!("Error stepping: instruction debugging not enabled");
                }
            }
            _ => {
                println!("Unknown command: {}. Type 'help' for available commands.", input);
            }
        }
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
}

/// Display instruction context around current position
fn display_instruction_context(engine: &DebuggerEngine, context_size: usize) {
    let context = engine.get_instruction_context(context_size);
    let formatted = Formatter::format_instruction_context(&context, context_size);
    println!("{}", formatted);
}

/// Display detailed instruction information
fn display_instruction_info(engine: &DebuggerEngine) {
    if let Some(state) = engine.state().lock().ok() {
        let ip = state.instruction_pointer();
        let step_mode = if ip.is_stepping() { Some(ip.step_mode()) } else { None };
        
        println!("{}", Formatter::format_instruction_pointer_state(
            ip.current_index(),
            ip.call_stack_depth(),
            step_mode,
            ip.is_stepping(),
        ));

        let stats = Formatter::format_instruction_stats(
            state.instructions().len(),
            ip.current_index(),
            state.step_count(),
        );
        println!("{}", stats);

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
