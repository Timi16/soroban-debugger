use crate::analyzer::upgrade::{CompatibilityReport, ExecutionDiff, UpgradeAnalyzer};
use crate::cli::args::{
    AnalyzeArgs, CompareArgs, GraphFormat, InspectArgs, InteractiveArgs, OptimizeArgs, ProfileArgs,
    RemoteArgs, ReplArgs, ReplayArgs, RunArgs, ScenarioArgs, ServerArgs, SymbolicArgs, TuiArgs,
    UpgradeCheckArgs, Verbosity,
};
use crate::debugger::engine::DebuggerEngine;
use crate::debugger::instruction_pointer::StepMode;
use crate::history::{HistoryManager, RunHistory};
use crate::inspector::events::{ContractEvent, EventInspector};
use crate::logging;
use crate::output::{OutputConfig, OutputWriter};
use crate::repeat::RepeatRunner;
use crate::runtime::executor::ContractExecutor;
use crate::simulator::SnapshotLoader;
use crate::ui::formatter::Formatter;
use crate::ui::tui::DebuggerUI;
use crate::{DebuggerError, Result};
use miette::WrapErr;
use std::fs;

fn print_info(message: impl AsRef<str>) {
    if !Formatter::is_quiet() {
        println!("{}", Formatter::info(message));
    }
}

fn print_success(message: impl AsRef<str>) {
    if !Formatter::is_quiet() {
        println!("{}", Formatter::success(message));
    }
}

fn print_warning(message: impl AsRef<str>) {
    if !Formatter::is_quiet() {
        println!("{}", Formatter::warning(message));
    }
}

/// Print the final contract return value — always shown regardless of verbosity.
fn print_result(message: impl AsRef<str>) {
    println!("{}", Formatter::success(message));
}

/// Print verbose-only detail — only shown when --verbose is active.
fn print_verbose(message: impl AsRef<str>) {
    if Formatter::is_verbose() {
        println!("{}", Formatter::info(message));
    }
}

/// Placeholder for instruction stepping
fn run_instruction_stepping(
    _engine: &mut DebuggerEngine,
    _function: &str,
    _args: Option<&str>,
) -> Result<()> {
    print_info("Instruction stepping is not yet fully implemented");
    Ok(())
}

/// Parse step mode from string
fn parse_step_mode(mode: &str) -> StepMode {
    match mode.to_lowercase().as_str() {
        "into" => StepMode::StepInto,
        "over" => StepMode::StepOver,
        "out" => StepMode::StepOut,
        "block" => StepMode::StepBlock,
        _ => StepMode::StepInto, // Default
    }
}

/// Display mock call log
fn display_mock_call_log(calls: &[crate::runtime::executor::MockCallEntry]) {
    if calls.is_empty() {
        return;
    }
    print_info("\n--- Mock Contract Calls ---");
    for (i, entry) in calls.iter().enumerate() {
        let status = if entry.mocked { "MOCKED" } else { "REAL" };
        print_info(format!(
            "{}. {} {} (args: {}) -> {}",
            i + 1,
            status,
            entry.function,
            entry.args_count,
            if entry.returned.is_some() {
                "returned"
            } else {
                "pending"
            }
        ));
    }
}

/// Execute batch mode with parallel execution
fn run_batch(args: &RunArgs, batch_file: &std::path::Path) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_bytes = fs::read(&args.contract).map_err(|e| {
        DebuggerError::WasmLoadError(format!(
            "Failed to read WASM file at {:?}: {}",
            args.contract, e
        ))
    })?;

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
        logging::log_display(
            serde_json::to_string_pretty(&output).map_err(|e| {
                DebuggerError::FileError(format!("Failed to serialize output: {}", e))
            })?,
            logging::LogLevel::Info,
        );
    }

    logging::log_execution_complete(&format!("{}/{} passed", summary.passed, summary.total));

    if summary.failed > 0 || summary.errors > 0 {
        return Err(DebuggerError::ExecutionError(format!(
            "Batch execution completed with failures: {} failed, {} errors",
            summary.failed, summary.errors
        ))
        .into());
    }

    Ok(())
}

/// Execute the run command.
#[tracing::instrument(skip_all, fields(contract = ?args.contract, function = args.function))]
pub fn run(args: RunArgs, verbosity: Verbosity) -> Result<()> {
    // Initialize output writer
    let mut output_writer = OutputWriter::new(args.save_output.as_deref(), args.append)?;

    // Handle batch execution mode
    if let Some(batch_file) = &args.batch_args {
        return run_batch(&args, batch_file);
    }

    if args.dry_run {
        return run_dry_run(&args);
    }

    print_info(format!("Loading contract: {:?}", args.contract));
    output_writer.write(&format!("Loading contract: {:?}", args.contract))?;
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err(crate::DebuggerError::ChecksumMismatch {
                expected: expected.clone(),
                actual: wasm_hash.clone(),
            }
            .into());
        }
    }

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));
    output_writer.write(&format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ))?;

    if args.verbose || verbosity == Verbosity::Verbose {
        print_verbose(format!("SHA-256: {}", wasm_hash));
        output_writer.write(&format!("SHA-256: {}", wasm_hash))?;
        if args.expected_hash.is_some() {
            print_verbose("Checksum verified ✓");
            output_writer.write("Checksum verified ✓")?;
        }
    }

    logging::log_contract_loaded(wasm_bytes.len());

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("\nLoading network snapshot: {:?}", snapshot_path));
        output_writer.write(&format!("Loading network snapshot: {:?}", snapshot_path))?;
        logging::log_loading_snapshot(&snapshot_path.to_string_lossy());
        let loader = SnapshotLoader::from_file(snapshot_path)?;
        let loaded_snapshot = loader.apply_to_environment()?;
        output_writer.write(&loaded_snapshot.format_summary())?;
        logging::log_display(loaded_snapshot.format_summary(), logging::LogLevel::Info);
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

    // Import storage if specified
    if let Some(import_path) = &args.import_storage {
        print_info(format!("Importing storage from: {:?}", import_path));
        let imported = crate::inspector::storage::StorageState::import_from_file(import_path)?;
        print_success(format!("Imported {} storage entries", imported.len()));
        initial_storage = Some(serde_json::to_string(&imported).map_err(|e| {
            DebuggerError::StorageError(format!("Failed to serialize imported storage: {}", e))
        })?);
    }

    if let Some(n) = args.repeat {
        logging::log_repeat_execution(&args.function, n as usize);
        let runner = RepeatRunner::new(wasm_bytes, args.breakpoint, initial_storage);
        let stats = runner.run(&args.function, parsed_args.as_deref(), n)?;
        stats.display();
        return Ok(());
    }

    print_info("\nStarting debugger...");
    output_writer.write("Starting debugger...")?;
    print_info(format!("Function: {}", args.function));
    output_writer.write(&format!("Function: {}", args.function))?;
    if let Some(ref parsed) = parsed_args {
        print_info(format!("Arguments: {}", parsed));
        output_writer.write(&format!("Arguments: {}", parsed))?;
    }
    logging::log_execution_start(&args.function, parsed_args.as_deref());

    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;
    executor.set_timeout(args.timeout);

    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }
    if !args.mock.is_empty() {
        executor.set_mock_specs(&args.mock)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint);

    // Server and remote modes are not yet implemented
    if args.server {
        return Err(DebuggerError::ExecutionError(
            "Server mode not yet implemented in run command".to_string(),
        )
        .into());
    }

    if args.remote.is_some() {
        return Err(DebuggerError::ExecutionError(
            "Remote mode not yet implemented in run command".to_string(),
        )
        .into());
    }

    // Execute locally with debugging
    println!("\n--- Execution Start ---\n");
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
    output_writer.write("\n--- Execution Start ---\n")?;
    let storage_before = engine.executor().get_storage_snapshot()?;
    let result = engine.execute(&args.function, parsed_args.as_deref())?;
    let storage_after = engine.executor().get_storage_snapshot()?;
    print_success("\n--- Execution Complete ---\n");
    output_writer.write("\n--- Execution Complete ---\n")?;
    print_result(format!("Result: {:?}", result));
    output_writer.write(&format!("Result: {:?}", result))?;
    logging::log_execution_complete(&result);

    // Generate test if requested
    if let Some(test_path) = &args.generate_test {
        if let Some(record) = engine.executor().last_execution() {
            print_info(format!("\nGenerating unit test: {:?}", test_path));
            let test_code = crate::codegen::TestGenerator::generate(record, &args.contract)?;
            crate::codegen::TestGenerator::write_to_file(test_path, &test_code, args.overwrite)?;
            print_success(format!(
                "Unit test generated successfully at {:?}",
                test_path
            ));
        } else {
            print_warning("No execution record found to generate test.");
        }
    }

    let storage_diff = crate::inspector::storage::StorageInspector::compute_diff(
        &storage_before,
        &storage_after,
        &args.alert_on_change,
    );
    if !storage_diff.is_empty() || !args.alert_on_change.is_empty() {
        print_info("\n--- Storage Changes ---");
        crate::inspector::storage::StorageInspector::display_diff(&storage_diff);
    }

    // Storage access pattern analysis
    if !args.json {
        engine.executor().storage_inspector().display_access_report();
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
    let _json_memory_summary = engine.executor().last_memory_summary().cloned();

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

    let mut json_events = None;
    if args.show_events || !args.event_filter.is_empty() || args.filter_topic.is_some() {
        print_info("\n--- Events ---");

        // Attempt to read raw events from executor
        let raw_events = engine.executor().get_events()?;

        // Convert runtime event objects into our inspector::events::ContractEvent via serde translation.
        // This is a generic, safe conversion as long as runtime events are serializable with sensible fields.
        let converted_events: Vec<ContractEvent> =
            match serde_json::to_value(&raw_events).and_then(serde_json::from_value) {
                Ok(evts) => evts,
                Err(e) => {
                    // If conversion fails, fall back to attempting to stringify each raw event for display.
                    print_warning(format!(
                        "Failed to convert runtime events for structured display: {}",
                        e
                    ));
                    // Fallback: attempt a best-effort stringification
                    let fallback: Vec<ContractEvent> = raw_events
                        .into_iter()
                        .map(|r| ContractEvent {
                            contract_id: None,
                            topics: vec![],
                            data: format!("{:?}", r),
                        })
                        .collect();
                    fallback
                }
            };

        // Determine filter: prefer repeatable --event-filter, fallback to legacy --filter-topic
        let filter_opt = if !args.event_filter.is_empty() {
            Some(args.event_filter.join(","))
        } else {
            args.filter_topic.clone()
        };

        let filtered_events = if let Some(ref filt) = filter_opt {
            EventInspector::filter_events(&converted_events, filt)
        } else {
            converted_events.clone()
        };

        if filtered_events.is_empty() {
            print_warning("No events captured.");
        } else {
            // Display events in readable form
            let lines = EventInspector::format_events(&filtered_events);
            for line in &lines {
                print_info(line);
            }
        }

        json_events = Some(filtered_events);
    }

    if !args.storage_filter.is_empty() {
        let storage_filter = crate::inspector::storage::StorageFilter::new(&args.storage_filter)
            .map_err(|e| DebuggerError::StorageError(format!("Invalid storage filter: {}", e)))?;

        print_info("\n--- Storage ---");
        let mut inspector = crate::inspector::StorageInspector::new();
        for (k, v) in &storage_after {
            inspector.set(k, v);
        }
        inspector.display_filtered(&storage_filter);
    }

    let mut json_auth = None;
    if args.show_auth {
        let auth_tree = engine.executor().get_auth_tree()?;
        if args.json {
            // JSON mode: print the auth tree inline (will also be included in
            // the combined JSON object further below).
            let json_output = crate::inspector::auth::AuthInspector::to_json(&auth_tree)?;
            logging::log_display(json_output, logging::LogLevel::Info);
        } else {
            print_info("\n--- Authorization Tree ---");
            crate::inspector::auth::AuthInspector::display_with_summary(&auth_tree);
        }
        json_auth = Some(auth_tree);
    }

    let mut json_ledger = None;
    if args.show_ledger {
        print_info("\n--- Ledger Entries ---");
        let mut ledger_inspector = crate::inspector::ledger::LedgerEntryInspector::new();
        ledger_inspector.set_ttl_warning_threshold(args.ttl_warning_threshold);

        match engine.executor_mut().finish() {
            Ok((footprint, storage)) => {
                let mut footprint_map = std::collections::HashMap::new();
                for (k, v) in &footprint.0 {
                    footprint_map.insert(k.clone(), *v);
                }

                for (key, val_opt) in &storage.map {
                    if let Some(access_type) = footprint_map.get(key) {
                        if let Some((entry, ttl)) = val_opt {
                            let key_str = format!("{:?}", **key);
                            let storage_type =
                                if key_str.contains("Temporary") || key_str.contains("temporary") {
                                    crate::inspector::ledger::StorageType::Temporary
                                } else if key_str.contains("Instance")
                                    || key_str.contains("instance")
                                    || key_str.contains("LedgerKeyContractInstance")
                                {
                                    crate::inspector::ledger::StorageType::Instance
                                } else {
                                    crate::inspector::ledger::StorageType::Persistent
                                };

                            use soroban_env_host::storage::AccessType;
                            let is_read = true; // Everything in the footprint is at least read
                            let is_write = matches!(*access_type, AccessType::ReadWrite);

                            ledger_inspector.add_entry(
                                format!("{:?}", **key),
                                format!("{:?}", **entry),
                                storage_type,
                                ttl.unwrap_or(0),
                                is_read,
                                is_write,
                            );
                        }
                    }
                }
            }
            Err(e) => {
                print_warning(format!("Failed to extract ledger footprint: {}", e));
            }
        }

        ledger_inspector.display();
        ledger_inspector.display_warnings();
        json_ledger = Some(ledger_inspector);
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
            "sha256": wasm_hash,
            "alerts": storage_diff.triggered_alerts,
        });

        if let Some(ref events) = json_events {
            output["events"] = EventInspector::to_json_value(events);
        }
        if let Some(auth_tree) = json_auth {
            output["auth"] = crate::inspector::auth::AuthInspector::to_json_value(&auth_tree);
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
        if let Some(ref ledger) = json_ledger {
            output["ledger_entries"] = ledger.to_json();
        }

        output["storage_access"] = engine.executor().storage_inspector().analyze_access_patterns().to_json();

        logging::log_display(
            serde_json::to_string_pretty(&output).map_err(|e| {
                DebuggerError::FileError(format!("Failed to serialize output: {}", e))
            })?,
            logging::LogLevel::Info,
        );
    }

    if let Some(trace_path) = &args.trace_output {
        print_info(format!("\nExporting execution trace to: {:?}", trace_path));

        let args_str = parsed_args
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());

        let trace_events =
            json_events.unwrap_or_else(|| engine.executor().get_events().unwrap_or_default());

        let trace = build_execution_trace(
            &args.function,
            args.contract.to_string_lossy().as_ref(),
            args_str,
            &storage_after,
            &result,
            budget,
            engine.executor(),
            &trace_events,
            usize::MAX,
        );

        if let Ok(json) = trace.to_json() {
            if let Err(e) = std::fs::write(trace_path, json) {
                print_warning(format!("Failed to write trace to {:?}: {}", trace_path, e));
            } else {
                print_success(format!("Successfully exported trace to {:?}", trace_path));
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_execution_trace(
    function: &str,
    contract_path: &str,
    args_str: Option<String>,
    storage_after: &std::collections::HashMap<String, String>,
    result: &str,
    budget: crate::inspector::budget::BudgetInfo,
    executor: &ContractExecutor,
    events: &[crate::inspector::events::ContractEvent],
    replay_until: usize,
) -> crate::compare::ExecutionTrace {
    let mut trace_storage = std::collections::BTreeMap::new();
    for (k, v) in storage_after {
        if let Ok(val) = serde_json::from_str(v) {
            trace_storage.insert(k.clone(), val);
        } else {
            trace_storage.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
    }

    let return_val = serde_json::from_str(result)
        .unwrap_or_else(|_| serde_json::Value::String(result.to_string()));

    let mut call_sequence = Vec::new();
    let mut depth = 0;

    call_sequence.push(crate::compare::trace::CallEntry {
        function: function.to_string(),
        args: args_str.clone(),
        depth,
    });

    if let Ok(diag_events) = executor.get_diagnostic_events() {
        for event in diag_events {
            // Stop building trace if we hit the replay limit
            if call_sequence.len() >= replay_until {
                break;
            }

            let event_str = format!("{:?}", event);
            if event_str.contains("ContractCall")
                || (event_str.contains("call") && event.contract_id.is_some())
            {
                depth += 1;
                call_sequence.push(crate::compare::trace::CallEntry {
                    function: "nested_call".to_string(),
                    args: None,
                    depth,
                });
            } else if (event_str.contains("ContractReturn") || event_str.contains("return"))
                && depth > 0
            {
                depth -= 1;
            }
        }
    }

    let mut trace_events = Vec::new();
    for e in events {
        trace_events.push(crate::compare::trace::EventEntry {
            contract_id: e.contract_id.clone(),
            topics: e.topics.clone(),
            data: Some(e.data.clone()),
        });
    }

    crate::compare::ExecutionTrace {
        label: Some(format!("Execution of {} on {}", function, contract_path)),
        contract: Some(contract_path.to_string()),
        function: Some(function.to_string()),
        args: args_str,
        storage: trace_storage,
        budget: Some(crate::compare::trace::BudgetTrace {
            cpu_instructions: budget.cpu_instructions,
            memory_bytes: budget.memory_bytes,
            cpu_limit: None,
            memory_limit: None,
        }),
        return_value: Some(return_val),
        call_sequence,
        events: trace_events,
    }
}

/// Execute run command in dry-run mode.
fn run_dry_run(args: &RunArgs) -> Result<()> {
    print_info(format!("[DRY RUN] Loading contract: {:?}", args.contract));

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err(crate::DebuggerError::ChecksumMismatch {
                expected: expected.clone(),
                actual: wasm_hash.clone(),
            }
            .into());
        }
    }

    print_success(format!(
        "[DRY RUN] Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    if args.verbose {
        print_verbose(format!("[DRY RUN] SHA-256: {}", wasm_hash));
        if args.expected_hash.is_some() {
            print_verbose("[DRY RUN] Checksum verified ✓");
        }
    }

    print_info("[DRY RUN] Skipping execution");

    Ok(())
}

/// Get instruction counts from the debugger engine
#[allow(dead_code)]
fn get_instruction_counts(
    engine: &DebuggerEngine,
) -> Option<crate::runtime::executor::InstructionCounts> {
    // Try to get instruction counts from the executor
    engine.executor().get_instruction_counts().ok()
}

/// Display instruction counts per function in a formatted table
#[allow(dead_code)]
fn display_instruction_counts(counts: &crate::runtime::executor::InstructionCounts) {
    if counts.function_counts.is_empty() {
        return;
    }

    print_info("\n--- Instruction Count per Function ---");

    // Calculate percentages
    let percentages: Vec<f64> = counts
        .function_counts
        .iter()
        .map(|(_, count)| {
            if counts.total > 0 {
                (*count as f64 / counts.total as f64) * 100.0
            } else {
                0.0
            }
        })
        .collect();

    // Find max widths for alignment
    let max_func_width = counts
        .function_counts
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(20);
    let max_count_width = counts
        .function_counts
        .iter()
        .map(|(_, count)| count.to_string().len())
        .max()
        .unwrap_or(10);

    // Print header
    let header = format!(
        "{:<width1$} | {:>width2$} | {:>width3$}",
        "Function",
        "Instructions",
        "Percentage",
        width1 = max_func_width,
        width2 = max_count_width,
        width3 = 10
    );
    print_info(&header);
    print_info("-".repeat(header.len()));

    // Print rows
    for ((func_name, count), percentage) in counts.function_counts.iter().zip(percentages.iter()) {
        let row = format!(
            "{:<width1$} | {:>width2$} | {:>7.2}%",
            func_name,
            count,
            percentage,
            width1 = max_func_width,
            width2 = max_count_width
        );
        print_info(&row);
    }
}

/// Execute the upgrade-check command
pub fn upgrade_check(args: UpgradeCheckArgs) -> Result<()> {
    println!("Loading old contract: {:?}", args.old);
    let old_wasm = fs::read(&args.old)
        .map_err(|e| miette::miette!("Failed to read old WASM file {:?}: {}", args.old, e))?;

    println!("Loading new contract: {:?}", args.new);
    let new_wasm = fs::read(&args.new)
        .map_err(|e| miette::miette!("Failed to read new WASM file {:?}: {}", args.new, e))?;

    // Optionally run test inputs against both versions
    let execution_diffs = if let Some(inputs_json) = &args.test_inputs {
        run_test_inputs(inputs_json, &old_wasm, &new_wasm)?
    } else {
        Vec::new()
    };

    let old_path = args.old.to_string_lossy().to_string();
    let new_path = args.new.to_string_lossy().to_string();

    let report =
        UpgradeAnalyzer::analyze(&old_wasm, &new_wasm, &old_path, &new_path, execution_diffs)?;

    let output = match args.output.as_str() {
        "json" => serde_json::to_string_pretty(&report)
            .map_err(|e| miette::miette!("Failed to serialize report: {}", e))?,
        _ => format_text_report(&report),
    };

    if let Some(out_file) = &args.output_file {
        fs::write(out_file, &output)
            .map_err(|e| miette::miette!("Failed to write report to {:?}: {}", out_file, e))?;
        println!("Report written to {:?}", out_file);
    } else {
        println!("{}", output);
    }

    if !report.is_compatible {
        return Err(miette::miette!(
            "Contracts are not compatible: {} breaking change(s) detected",
            report.breaking_changes.len()
        ));
    }

    Ok(())
}

/// Execute the interactive command.
#[tracing::instrument(skip_all, fields(contract = ?args.contract))]
pub fn interactive(args: InteractiveArgs, verbosity: Verbosity) -> Result<()> {
    print_info(format!(
        "Starting interactive debugger for: {:?}",
        args.contract
    ));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err(crate::DebuggerError::ChecksumMismatch {
                expected: expected.clone(),
                actual: wasm_hash.clone(),
            }
            .into());
        }
    }

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    if verbosity == Verbosity::Verbose {
        print_verbose(format!("SHA-256: {}", wasm_hash));
        if args.expected_hash.is_some() {
            print_verbose("Checksum verified ✓");
        }
    }

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

/// Launch the full-screen TUI dashboard.
pub fn tui(args: TuiArgs, _verbosity: Verbosity) -> Result<()> {
    let wasm_bytes = fs::read(&args.contract).map_err(|e| {
        DebuggerError::WasmLoadError(format!(
            "Failed to read WASM file: {:?}. Error: {}",
            args.contract, e
        ))
    })?;

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

    let mut engine = DebuggerEngine::new(executor, args.breakpoint);

    // Pre-execute so live data is available immediately in the dashboard
    let _ = engine.execute(&args.function, parsed_args.as_deref());

    crate::ui::run_dashboard(engine, &args.function)?;

    Ok(())
}

/// Execute the inspect command.
#[tracing::instrument(skip_all, fields(contract = ?args.contract))]
pub fn inspect(args: InspectArgs, verbosity: Verbosity) -> Result<()> {
    print_info(format!("Inspecting contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err(crate::DebuggerError::ChecksumMismatch {
                expected: expected.clone(),
                actual: wasm_hash.clone(),
            }
            .into());
        }
    }

    if verbosity == Verbosity::Verbose {
        print_verbose(format!("SHA-256: {}", wasm_hash));
        if args.expected_hash.is_some() {
            print_verbose("Checksum verified ✓");
        }
    }

    let module_info = crate::utils::wasm::get_module_info(&wasm_bytes)?;

    logging::log_display(format!("\n{}", "=".repeat(54)), logging::LogLevel::Info);
    logging::log_display("  Soroban Contract Inspector", logging::LogLevel::Info);
    logging::log_display("=".repeat(54), logging::LogLevel::Info);
    logging::log_display(
        format!("\n  File : {:?}", args.contract),
        logging::LogLevel::Info,
    );
    logging::log_display(
        format!("  Size : {} bytes", wasm_bytes.len()),
        logging::LogLevel::Info,
    );

    logging::log_display(format!("\n{}", "-".repeat(54)), logging::LogLevel::Info);
    logging::log_display("  Module Information", logging::LogLevel::Info);
    logging::log_display("-".repeat(54), logging::LogLevel::Info);
    logging::log_display(
        format!("  Types      : {}", module_info.type_count),
        logging::LogLevel::Info,
    );
    logging::log_display(
        format!("  Functions  : {}", module_info.function_count),
        logging::LogLevel::Info,
    );
    logging::log_display(
        format!("  Exports    : {}", module_info.export_count),
        logging::LogLevel::Info,
    );

    if args.functions {
        logging::log_display(format!("\n{}", "-".repeat(54)), logging::LogLevel::Info);
        logging::log_display("  Exported Functions", logging::LogLevel::Info);
        logging::log_display("-".repeat(54), logging::LogLevel::Info);

        let functions = crate::utils::wasm::parse_functions(&wasm_bytes)?;
        if functions.is_empty() {
            logging::log_display("  (No exported functions found)", logging::LogLevel::Info);
        } else {
            for function in functions {
                logging::log_display(
                    format!("  {} {}", OutputConfig::to_ascii("•"), function),
                    logging::LogLevel::Info,
                );
            }
        }
    }

    if args.dependency_graph {
        logging::log_display(
            format!("\n{}", OutputConfig::rule_line(54)),
            logging::LogLevel::Info,
        );
        logging::log_display("  Contract Dependency Graph", logging::LogLevel::Info);
        logging::log_display(
            format!("  {}", OutputConfig::rule_line(52)),
            logging::LogLevel::Info,
        );

        let calls = crate::utils::wasm::parse_cross_contract_calls(&wasm_bytes)?;
        if calls.is_empty() {
            logging::log_display(
                "  (No cross-contract call instructions detected)",
                logging::LogLevel::Info,
            );
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

            logging::log_display("\nDOT:", logging::LogLevel::Info);
            logging::log_display(graph.to_dot(), logging::LogLevel::Info);
            logging::log_display("\nMermaid:", logging::LogLevel::Info);
            logging::log_display(graph.to_mermaid(), logging::LogLevel::Info);
        }
    }

    if args.metadata {
        logging::log_display(format!("\n{}", "-".repeat(54)), logging::LogLevel::Info);
        logging::log_display("  Contract Metadata", logging::LogLevel::Info);
        logging::log_display("-".repeat(54), logging::LogLevel::Info);

        let metadata = crate::utils::wasm::extract_contract_metadata(&wasm_bytes)?;
        if metadata.is_empty() {
            logging::log_display("  (No embedded metadata found)", logging::LogLevel::Info);
        } else {
            if let Some(version) = metadata.contract_version {
                logging::log_display(
                    format!("  Contract version      : {}", version),
                    logging::LogLevel::Info,
                );
            }
            if let Some(sdk) = metadata.sdk_version {
                logging::log_display(
                    format!("  Soroban SDK version   : {}", sdk),
                    logging::LogLevel::Info,
                );
            }
            if let Some(build_date) = metadata.build_date {
                logging::log_display(
                    format!("  Build date            : {}", build_date),
                    logging::LogLevel::Info,
                );
            }
            if let Some(author) = metadata.author {
                logging::log_display(
                    format!("  Author / organization : {}", author),
                    logging::LogLevel::Info,
                );
            }
            if let Some(desc) = metadata.description {
                logging::log_display(
                    format!("  Description           : {}", desc),
                    logging::LogLevel::Info,
                );
            }
            if let Some(impl_notes) = metadata.implementation {
                logging::log_display(
                    format!("  Implementation notes  : {}", impl_notes),
                    logging::LogLevel::Info,
                );
            }
        }
    }

    logging::log_display(format!("\n{}", "=".repeat(54)), logging::LogLevel::Info);
    Ok(())
}

/// Execute the analyze command.
#[tracing::instrument(skip_all, fields(contract = ?args.contract, function = ?args.function))]
pub fn analyze(args: AnalyzeArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Analyzing contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));
    Ok(())
}

/// Run test inputs against both WASM versions and collect diffs
fn run_test_inputs(
    inputs_json: &str,
    old_wasm: &[u8],
    new_wasm: &[u8],
) -> Result<Vec<ExecutionDiff>> {
    let inputs: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(inputs_json)
            .map_err(|e| miette::miette!("Invalid --test-inputs JSON (expected an object mapping function names to arg arrays): {}", e))?;

    let mut diffs = Vec::new();

    for (func_name, args_val) in &inputs {
        let args_str = args_val.to_string();

        let old_result = invoke_wasm(old_wasm, func_name, &args_str);
        let new_result = invoke_wasm(new_wasm, func_name, &args_str);

        let outputs_match = old_result == new_result;
        diffs.push(ExecutionDiff {
            function: func_name.clone(),
            args: args_str,
            old_result,
            new_result,
            outputs_match,
        });
    }

    Ok(diffs)
}

/// Invoke a function on a WASM contract and return a string representation of the result
fn invoke_wasm(wasm: &[u8], function: &str, args: &str) -> String {
    match ContractExecutor::new(wasm.to_vec()) {
        Err(e) => format!("Err(executor: {})", e),
        Ok(executor) => {
            let mut engine = DebuggerEngine::new(executor, vec![]);
            let parsed = if args == "null" || args == "[]" {
                None
            } else {
                Some(args.to_string())
            };
            match engine.execute(function, parsed.as_deref()) {
                Ok(val) => format!("Ok({:?})", val),
                Err(e) => format!("Err({})", e),
            }
        }
    }
}

/// Format a compatibility report as human-readable text
fn format_text_report(report: &CompatibilityReport) -> String {
    let mut out = String::new();

    out.push_str("Contract Upgrade Compatibility Report\n");
    out.push_str("======================================\n");
    out.push_str(&format!("Old: {}\n", report.old_wasm_path));
    out.push_str(&format!("New: {}\n", report.new_wasm_path));
    out.push('\n');

    let status = if report.is_compatible {
        "COMPATIBLE"
    } else {
        "INCOMPATIBLE"
    };
    out.push_str(&format!("Status: {}\n", status));

    out.push('\n');
    out.push_str(&format!(
        "Breaking Changes ({}):\n",
        report.breaking_changes.len()
    ));
    if report.breaking_changes.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for change in &report.breaking_changes {
            out.push_str(&format!("  {}\n", change));
        }
    }

    out.push('\n');
    out.push_str(&format!(
        "Non-Breaking Changes ({}):\n",
        report.non_breaking_changes.len()
    ));
    if report.non_breaking_changes.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for change in &report.non_breaking_changes {
            out.push_str(&format!("  {}\n", change));
        }
    }

    if !report.execution_diffs.is_empty() {
        out.push('\n');
        out.push_str(&format!(
            "Execution Diffs ({}):\n",
            report.execution_diffs.len()
        ));
        for diff in &report.execution_diffs {
            let match_str = if diff.outputs_match {
                "MATCH"
            } else {
                "MISMATCH"
            };
            out.push_str(&format!(
                "  {} args={} OLD={} NEW={} [{}]\n",
                diff.function, diff.args, diff.old_result, diff.new_result, match_str
            ));
        }
    }

    out.push('\n');
    let old_names: Vec<&str> = report
        .old_functions
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    let new_names: Vec<&str> = report
        .new_functions
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    out.push_str(&format!(
        "Old Functions ({}): {}\n",
        old_names.len(),
        old_names.join(", ")
    ));
    out.push_str(&format!(
        "New Functions ({}): {}\n",
        new_names.len(),
        new_names.join(", ")
    ));

    out
}

/// Parse JSON arguments with validation.
pub fn parse_args(json: &str) -> Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(json).map_err(|e| {
        DebuggerError::InvalidArguments(format!(
            "Failed to parse JSON arguments: {}. Error: {}",
            json, e
        ))
    })?;

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
    serde_json::from_str::<serde_json::Value>(json).map_err(|e| {
        DebuggerError::StorageError(format!(
            "Failed to parse JSON storage: {}. Error: {}",
            json, e
        ))
    })?;
    Ok(json.to_string())
}

/// Execute the optimize command.
pub fn optimize(args: OptimizeArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!(
        "Analyzing contract for gas optimization: {:?}",
        args.contract
    ));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err(crate::DebuggerError::ChecksumMismatch {
                expected: expected.clone(),
                actual: wasm_hash.clone(),
            }
            .into());
        }
    }

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    if _verbosity == Verbosity::Verbose {
        print_verbose(format!("SHA-256: {}", wasm_hash));
        if args.expected_hash.is_some() {
            print_verbose("Checksum verified ✓");
        }
    }

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
                logging::log_display(
                    format!(
                        "    CPU: {} instructions, Memory: {} bytes, Time: {} ms",
                        profile.total_cpu, profile.total_memory, profile.wall_time_ms
                    ),
                    logging::LogLevel::Info,
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
        fs::write(output_path, &markdown).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write report to {:?}: {}",
                output_path, e
            ))
        })?;
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
    logging::log_display(
        format!("Profiling contract execution: {:?}", args.contract),
        logging::LogLevel::Info,
    );

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err(crate::DebuggerError::ChecksumMismatch {
                expected: expected.clone(),
                actual: wasm_hash.clone(),
            }
            .into());
        }
    }

    logging::log_display(
        format!("Contract loaded successfully ({} bytes)", wasm_bytes.len()),
        logging::LogLevel::Info,
    );

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

    logging::log_display(
        format!("\nRunning function: {}", args.function),
        logging::LogLevel::Info,
    );
    if let Some(ref a) = parsed_args {
        logging::log_display(format!("Args: {}", a), logging::LogLevel::Info);
    }

    let _profile = optimizer.analyze_function(&args.function, parsed_args.as_deref())?;

    let contract_path_str = args.contract.to_string_lossy().to_string();
    let report = optimizer.generate_report(&contract_path_str);

    // Hotspot summary first
    logging::log_display(
        format!("\n{}", report.format_hotspots()),
        logging::LogLevel::Info,
    );

    // Then detailed suggestions (markdown format)
    let markdown = optimizer.generate_markdown_report(&report);

    if let Some(output_path) = &args.output {
        fs::write(output_path, &markdown).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write report to {:?}: {}",
                output_path, e
            ))
        })?;
        logging::log_display(
            format!("\nProfile report written to: {:?}", output_path),
            logging::LogLevel::Info,
        );
    } else {
        logging::log_display(format!("\n{}", markdown), logging::LogLevel::Info);
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
        fs::write(output_path, &rendered).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write report to {:?}: {}",
                output_path, e
            ))
        })?;
        print_success(format!("Comparison report written to: {:?}", output_path));
    } else {
        logging::log_display(rendered, logging::LogLevel::Info);
    }

    Ok(())
}

/// Execute the replay command.
pub fn replay(args: ReplayArgs, verbosity: Verbosity) -> Result<()> {
    print_info(format!("Loading trace file: {:?}", args.trace_file));
    let original_trace = crate::compare::ExecutionTrace::from_file(&args.trace_file)?;

    // Determine which contract to use
    let contract_path = if let Some(path) = &args.contract {
        path.clone()
    } else if let Some(contract_str) = &original_trace.contract {
        std::path::PathBuf::from(contract_str)
    } else {
        return Err(DebuggerError::ExecutionError(
            "No contract path specified and trace file does not contain contract path".to_string(),
        )
        .into());
    };

    print_info(format!("Loading contract: {:?}", contract_path));
    let wasm_bytes = fs::read(&contract_path).map_err(|e| {
        DebuggerError::WasmLoadError(format!(
            "Failed to read WASM file at {:?}: {}",
            contract_path, e
        ))
    })?;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    // Extract function and args from trace
    let function = original_trace.function.as_ref().ok_or_else(|| {
        DebuggerError::ExecutionError("Trace file does not contain function name".to_string())
    })?;

    let args_str = original_trace.args.as_deref();

    // Determine how many steps to replay
    let replay_steps = args.replay_until.unwrap_or(usize::MAX);
    let is_partial_replay = args.replay_until.is_some();

    if is_partial_replay {
        print_info(format!("Replaying up to step {}", replay_steps));
    } else {
        print_info("Replaying full execution");
    }

    print_info(format!("Function: {}", function));
    if let Some(a) = args_str {
        print_info(format!("Arguments: {}", a));
    }

    // Set up initial storage from trace
    let initial_storage = if !original_trace.storage.is_empty() {
        let storage_json = serde_json::to_string(&original_trace.storage).map_err(|e| {
            DebuggerError::StorageError(format!("Failed to serialize trace storage: {}", e))
        })?;
        Some(storage_json)
    } else {
        None
    };

    // Execute the contract
    print_info("\n--- Replaying Execution ---\n");
    let mut executor = ContractExecutor::new(wasm_bytes)?;

    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    let mut engine = DebuggerEngine::new(executor, vec![]);

    logging::log_execution_start(function, args_str);
    let replayed_result = engine.execute(function, args_str)?;

    print_success("\n--- Replay Complete ---\n");
    print_success(format!("Replayed Result: {:?}", replayed_result));
    logging::log_execution_complete(&replayed_result);

    // Build execution trace from the replay
    let storage_after = engine.executor().get_storage_snapshot()?;
    let trace_events = engine.executor().get_events().unwrap_or_default();
    let budget = crate::inspector::budget::BudgetInspector::get_cpu_usage(engine.executor().host());

    let replayed_trace = build_execution_trace(
        function,
        &contract_path.to_string_lossy(),
        args_str.map(|s| s.to_string()),
        &storage_after,
        &replayed_result,
        budget,
        engine.executor(),
        &trace_events,
        replay_steps,
    );

    // Truncate original_trace's call_sequence if needed to match replay_until
    let mut truncated_original = original_trace.clone();
    if truncated_original.call_sequence.len() > replay_steps {
        truncated_original.call_sequence.truncate(replay_steps);
    }

    // Compare results
    print_info("\n--- Comparison ---");
    let report = crate::compare::CompareEngine::compare(&truncated_original, &replayed_trace);
    let rendered = crate::compare::CompareEngine::render_report(&report);

    if let Some(output_path) = &args.output {
        std::fs::write(output_path, &rendered).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write report to {:?}: {}",
                output_path, e
            ))
        })?;
        print_success(format!("\nReplay report written to: {:?}", output_path));
    } else {
        logging::log_display(rendered, logging::LogLevel::Info);
    }

    if verbosity == Verbosity::Verbose {
        print_verbose("\n--- Call Sequence (Original) ---");
        for (i, call) in original_trace.call_sequence.iter().enumerate() {
            let indent = "  ".repeat(call.depth as usize);
            if let Some(args) = &call.args {
                print_verbose(format!("{}{}. {} ({})", indent, i, call.function, args));
            } else {
                print_verbose(format!("{}{}. {}", indent, i, call.function));
            }

            if is_partial_replay && i >= replay_steps {
                print_verbose(format!("{}... (stopped at step {})", indent, replay_steps));
                break;
            }
        }
    }

    Ok(())
}

/// Start debug server for remote connections
pub fn server(args: ServerArgs) -> Result<()> {
    print_info("Remote debugging server is not yet implemented in this build");
    print_info(format!("Requested port: {}", args.port));
    if args.token.is_some() {
        print_info("Token authentication would be enabled");
    }
    Err(DebuggerError::ExecutionError("Server mode not yet implemented".to_string()).into())
}

/// Connect to remote debug server
pub fn remote(args: RemoteArgs, _verbosity: Verbosity) -> Result<()> {
    print_info("Remote debugging client is not yet implemented in this build");
    print_info(format!("Requested connection to: {}", args.remote));
    Err(DebuggerError::ExecutionError("Remote mode not yet implemented".to_string()).into())
}

/// Run symbolic execution analysis
pub fn symbolic(args: SymbolicArgs, _verbosity: Verbosity) -> Result<()> {
    print_info("Symbolic mode is not yet implemented in this build");
    print_info(format!("Contract: {:?}", args.contract));
    Err(DebuggerError::ExecutionError("Symbolic mode not yet implemented".to_string()).into())
}

/// Run a scenario
pub fn scenario(args: ScenarioArgs, _verbosity: Verbosity) -> Result<()> {
    print_info("Scenario mode is not yet implemented in this build");
    print_info(format!("Scenario: {:?}", args.scenario));
    Err(DebuggerError::ExecutionError("Scenario mode not yet implemented".to_string()).into())
}

/// Launch the REPL
pub async fn repl(args: ReplArgs) -> Result<()> {
    print_info("REPL mode is not yet implemented in this build");
    print_info(format!("Contract: {:?}", args.contract));
    Err(DebuggerError::ExecutionError("REPL mode not yet implemented".to_string()).into())
}

/// Show budget trend chart
pub fn show_budget_trend(contract: Option<&str>, function: Option<&str>) -> Result<()> {
    print_info("Budget trend is not yet implemented in this build");
    if let Some(c) = contract {
        print_info(format!("Contract: {}", c));
    }
    if let Some(f) = function {
        print_info(format!("Function: {}", f));
    }
    Err(DebuggerError::ExecutionError("Budget trend not yet implemented".to_string()).into())
}
