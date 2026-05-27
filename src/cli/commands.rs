use crate::analyzer::symbolic::SymbolicConfig;
use crate::analyzer::upgrade::{CompatibilityReport, ExecutionDiff, UpgradeAnalyzer};
use crate::analyzer::{
    security::SecurityAnalyzer,
    symbolic::{build_replay_bundle, SymbolicAnalyzer},
};
use crate::cli::args::{
    AnalyzeArgs, CompareArgs, HistoryPruneArgs, InspectArgs, InteractiveArgs, OptimizeArgs,
    OutputFormat, ProfileArgs, RemoteAction, RemoteArgs, ReplArgs, ReplayArgs, RunArgs,
    ScenarioArgs, ServerArgs, SymbolicArgs, SymbolicProfile, TuiArgs, UpgradeCheckArgs, Verbosity,
};
use crate::cli::output::write_json_pretty_file;
use crate::debugger::engine::DebuggerEngine;
use crate::debugger::instruction_pointer::StepMode;
use crate::debugger::timeline::{
    TimelineDeltas, TimelineExport, TimelinePausePoint, TimelineRunInfo, TimelineStorageDelta,
    TimelineWarning, TIMELINE_EXPORT_SCHEMA_VERSION,
};
use crate::history::{HistoryManager, RunHistory};
use crate::inspector::events::{ContractEvent, EventInspector};
use crate::logging;
use crate::output::OutputWriter;
use crate::repeat::RepeatRunner;
use crate::repl::ReplConfig;
use crate::runtime::executor::ContractExecutor;
use crate::simulator::SnapshotLoader;
use crate::ui::formatter::Formatter;
use crate::ui::{run_dashboard, DebuggerUI};
use crate::{DebuggerError, Result};
use miette::WrapErr;
use std::fs;
use std::path::PathBuf;

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
    if !Formatter::is_quiet() {
        println!("{}", Formatter::success(message));
    }
}

/// Print verbose-only detail — only shown when --verbose is active.
fn print_verbose(message: impl AsRef<str>) {
    if Formatter::is_verbose() {
        println!("{}", Formatter::info(message));
    }
}

fn budget_trend_stats_or_err(records: &[RunHistory]) -> Result<crate::history::BudgetTrendStats> {
    crate::history::budget_trend_stats(records).ok_or_else(|| {
        DebuggerError::ExecutionError(
            "Failed to compute budget trend statistics for the selected dataset".to_string(),
        )
        .into()
    })
}

#[derive(serde::Serialize)]
struct DynamicAnalysisMetadata {
    function: String,
    args: Option<String>,
    result: Option<String>,
    trace_entries: usize,
}

#[derive(serde::Serialize)]
struct AnalyzeCommandOutput {
    findings: Vec<crate::analyzer::security::SecurityFinding>,
    dynamic_analysis: Option<DynamicAnalysisMetadata>,
    warnings: Vec<String>,
    suppressed_count: usize,
}

#[derive(serde::Serialize)]
struct SourceMapDiagnosticsCommandOutput {
    contract: String,
    source_map: crate::debugger::source_map::SourceMapInspectionReport,
}

fn render_symbolic_report(report: &crate::analyzer::symbolic::SymbolicReport) -> String {
    let mut lines = vec![
        format!("Function: {}", report.function),
        format!("Paths explored: {}", report.paths_explored),
        format!("Panics found: {}", report.panics_found),
        format!(
            "Replay token: {}",
            report
                .metadata
                .seed
                .map(|seed| seed.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "Budget: path_cap={}, input_combination_cap={}, timeout={}s",
            report.metadata.config.max_paths,
            report.metadata.config.max_input_combinations,
            report.metadata.config.timeout_secs
        ),
        format!(
            "Input combinations: generated={}, attempted={}, distinct_paths={}",
            report.metadata.generated_input_combinations,
            report.metadata.attempted_input_combinations,
            report.metadata.distinct_paths_recorded
        ),
        format!(
            "Coverage: {:.1}% (explored branch/function coverage)",
            report.metadata.coverage_fraction * 100.0
        ),
    ];

    if !report.metadata.uncovered_regions.is_empty() {
        lines.push(format!(
            "Uncovered regions: {}",
            report.metadata.uncovered_regions.join(", ")
        ));
    }

    if report.metadata.truncation_reasons.is_empty() {
        lines.push("Truncation: none".to_string());
    } else {
        lines.push(format!(
            "Truncation: {}",
            report.metadata.truncation_reasons.join("; ")
        ));
    }

    if report.paths.is_empty() {
        lines.push("No distinct execution paths were discovered.".to_string());
        return lines.join("\n");
    }

    lines.push(String::new());
    lines.push("Distinct paths:".to_string());

    for (idx, path) in report.paths.iter().enumerate() {
        let outcome = match (&path.return_value, &path.panic) {
            (Some(value), _) => format!("return {}", value),
            (_, Some(panic)) => format!("panic {}", panic),
            _ => "unknown".to_string(),
        };
        lines.push(format!(
            "  {}. inputs={} -> {}",
            idx + 1,
            path.inputs,
            outcome
        ));
    }

    lines.join("\n")
}

fn symbolic_profile_config(profile: SymbolicProfile) -> SymbolicConfig {
    match profile {
        SymbolicProfile::Fast => SymbolicConfig::fast(),
        SymbolicProfile::Balanced => SymbolicConfig::balanced(),
        SymbolicProfile::Deep => SymbolicConfig::deep(),
    }
}

fn symbolic_config_from_args(args: &SymbolicArgs) -> Result<SymbolicConfig> {
    let mut config = symbolic_profile_config(args.profile);
    if let Some(path_cap) = args.path_cap {
        config.max_paths = path_cap;
    }
    if let Some(input_cap) = args.input_combination_cap {
        config.max_input_combinations = input_cap;
    }
    if let Some(max_breadth) = args.max_breadth {
        config.max_breadth = max_breadth;
    }
    if let Some(timeout) = args.timeout {
        config.timeout_secs = timeout;
    }
    config.seed = args.seed.or(args.replay);
    if let Some(storage_seed_path) = &args.storage_seed {
        config.storage_seed = Some(fs::read_to_string(storage_seed_path).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to read storage seed file {:?}: {}",
                storage_seed_path, e
            ))
        })?);
    }

    Ok(config)
}

fn parse_min_severity(value: &str) -> Result<crate::analyzer::security::Severity> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Ok(crate::analyzer::security::Severity::Low),
        "medium" | "med" => Ok(crate::analyzer::security::Severity::Medium),
        "high" => Ok(crate::analyzer::security::Severity::High),
        other => Err(DebuggerError::InvalidArguments(format!(
            "Unsupported --min-severity '{}'. Use low, medium, or high.",
            other
        ))
        .into()),
    }
}

fn render_security_report(output: &AnalyzeCommandOutput) -> String {
    let mut lines = Vec::new();

    if let Some(dynamic) = &output.dynamic_analysis {
        lines.push(format!("Dynamic analysis function: {}", dynamic.function));
        if let Some(args) = &dynamic.args {
            lines.push(format!("Dynamic analysis args: {}", args));
        }
        if let Some(result) = &dynamic.result {
            lines.push(format!("Dynamic execution result: {}", result));
        }
        lines.push(format!(
            "Dynamic trace entries captured: {}",
            dynamic.trace_entries
        ));
        lines.push(String::new());
    }

    if !output.warnings.is_empty() {
        lines.push("Warnings:".to_string());
        for warning in &output.warnings {
            lines.push(format!("  - {}", warning));
        }
        lines.push(String::new());
    }

    if output.findings.is_empty() {
        lines.push("No security findings detected.".to_string());
        if output.suppressed_count > 0 {
            lines.push(format!(
                "({} findings were suppressed)",
                output.suppressed_count
            ));
        }
        return lines.join("\n");
    }

    lines.push(format!(
        "Findings: {} ({} suppressed)",
        output.findings.len(),
        output.suppressed_count
    ));
    for (idx, finding) in output.findings.iter().enumerate() {
        lines.push(format!(
            "  {}. [{:?}] {} at {}",
            idx + 1,
            finding.severity,
            finding.rule_id,
            finding.location
        ));
        lines.push(format!("     {}", finding.description));
        if let Some(confidence) = finding.confidence {
            lines.push(format!("     Confidence: {:.0}%", confidence * 100.0));
        }
        if let Some(rationale) = &finding.rationale {
            lines.push(format!("     Rationale: {}", rationale));
        }
        lines.push(format!("     Remediation: {}", finding.remediation));
    }

    lines.join("\n")
}

/// Run instruction-level stepping mode.
fn run_instruction_stepping(
    engine: &mut DebuggerEngine,
    function: &str,
    args: Option<&str>,
) -> Result<()> {
    logging::log_display(
        "\n=== Instruction Stepping Mode ===",
        logging::LogLevel::Info,
    );
    logging::log_display(
        "Type 'help' for available commands\n",
        logging::LogLevel::Info,
    );

    display_instruction_context(engine, 3);

    loop {
        print!("(step) > ");
        std::io::Write::flush(&mut std::io::stdout())
            .map_err(|e| DebuggerError::IoError(format!("Failed to flush stdout: {}", e)))?;

        let mut input = String::new();
        let bytes_read = std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| DebuggerError::IoError(format!("Failed to read line: {}", e)))?;
        if bytes_read == 0 {
            logging::log_display("Input stream closed.", logging::LogLevel::Info);
            break;
        }

        let input = input.trim().to_lowercase();
        let cmd = input.as_str();

        let result = match cmd {
            "n" | "next" | "s" | "step" | "into" | "" => engine.step_into(),
            "o" | "over" => engine.step_over(),
            "u" | "out" => engine.step_out(),
            "b" | "block" => engine.step_block(),
            "p" | "prev" | "back" => engine.step_back(),
            "c" | "continue" => {
                logging::log_display("Continuing execution...", logging::LogLevel::Info);
                engine.continue_execution()?;
                let res = engine.execute_without_breakpoints(function, args)?;
                logging::log_display(
                    format!("Execution completed. Result: {:?}", res),
                    logging::LogLevel::Info,
                );
                break;
            }
            "i" | "info" => {
                display_instruction_info(engine);
                continue;
            }
            "ctx" | "context" => {
                display_instruction_context(engine, 5);
                continue;
            }
            "h" | "help" => {
                logging::log_display(Formatter::format_stepping_help(), logging::LogLevel::Info);
                continue;
            }
            "q" | "quit" | "exit" => {
                logging::log_display(
                    "Exiting instruction stepping mode...",
                    logging::LogLevel::Info,
                );
                break;
            }
            _ => {
                logging::log_display(
                    format!("Unknown command: {cmd}. Type 'help' for available commands."),
                    logging::LogLevel::Info,
                );
                continue;
            }
        };

        match result {
            Ok(true) => display_instruction_context(engine, 3),
            Ok(false) => {
                let msg = if matches!(cmd, "p" | "prev" | "back") {
                    "Cannot step back: no previous instruction"
                } else {
                    "Cannot step: execution finished or error occurred"
                };
                logging::log_display(msg, logging::LogLevel::Info);
            }
            Err(e) => {
                logging::log_display(format!("Error stepping: {}", e), logging::LogLevel::Info)
            }
        }
    }

    Ok(())
}

fn display_instruction_context(engine: &DebuggerEngine, context_size: usize) {
    let context = engine.get_instruction_context(context_size);
    let formatted = Formatter::format_instruction_context(&context, context_size);
    logging::log_display(formatted, logging::LogLevel::Info);
}

fn display_instruction_info(engine: &DebuggerEngine) {
    if let Ok(state) = engine.state().lock() {
        let ip = state.instruction_pointer();
        let step_mode = if ip.is_stepping() {
            Some(ip.step_mode())
        } else {
            None
        };

        logging::log_display(
            Formatter::format_instruction_pointer_state(
                ip.current_index(),
                ip.call_stack_depth(),
                step_mode,
                ip.is_stepping(),
            ),
            logging::LogLevel::Info,
        );
        logging::log_display(
            Formatter::format_instruction_stats(
                state.instructions().len(),
                ip.current_index(),
                state.step_count(),
            ),
            logging::LogLevel::Info,
        );

        if let Some(inst) = state.current_instruction() {
            logging::log_display(
                format!(
                    "Current Instruction: {} (Offset: 0x{:08x}, Local index: {}, Control flow: {})",
                    inst.name(),
                    inst.offset,
                    inst.local_index,
                    inst.is_control_flow()
                ),
                logging::LogLevel::Info,
            );
        }
    } else {
        logging::log_display("Cannot access debug state", logging::LogLevel::Info);
    }
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
    let contract = args
        .contract
        .as_ref()
        .expect("contract is required for batch mode");
    let function = args
        .function
        .as_ref()
        .expect("function is required for batch mode");

    print_info(format!("Loading contract: {:?}", contract));
    logging::log_loading_contract(&contract.to_string_lossy());

    let wasm_bytes = fs::read(contract).map_err(|e| {
        DebuggerError::WasmLoadError(format!("Failed to read WASM file at {:?}: {}", contract, e))
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
        function
    ));
    logging::log_execution_start(function, None);

    let executor = crate::batch::BatchExecutor::new(wasm_bytes, function.clone())?;
    let results = executor.execute_batch(batch_items)?;
    let summary = crate::batch::BatchExecutor::summarize(&results);

    crate::batch::BatchExecutor::display_results(&results, &summary);

    if args.is_json_output() {
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
    // Start debug server if requested
    if args.server {
        return server(ServerArgs {
            host: args.host,
            port: args.port,
            token: args.token,
            tls_cert: args.tls_cert,
            tls_key: args.tls_key,
            repeat: args.repeat,
            storage_filter: args.storage_filter,
            show_events: args.show_events,
            event_filter: args.event_filter,
            mock: args.mock,
        });
    }

    // Remote execution/ping path.
    if let Some(remote_addr) = &args.remote {
        return remote(
            RemoteArgs {
                remote: remote_addr.clone(),
                token: args.token.clone(),
                contract: args.contract.clone(),
                function: args.function.clone(),
                tls_cert: args.tls_cert.clone(),
                tls_key: args.tls_key.clone(),
                tls_ca: None,
                session_label: None,
                args: args.args.clone(),
                connect_timeout_ms: 10000,
                timeout_ms: 30000,
                inspect_timeout_ms: None,
                storage_timeout_ms: None,
                retry_attempts: 3,
                retry_base_delay_ms: 200,
                retry_max_delay_ms: 2000,
                action: None,
            },
            verbosity,
        );
    }

    // Initialize output writer
    let mut output_writer = OutputWriter::new(args.save_output.as_deref(), args.append)?;

    // Handle batch execution mode
    if let Some(batch_file) = &args.batch_args {
        return run_batch(&args, batch_file);
    }

    if args.dry_run {
        return run_dry_run(&args);
    }

    let contract = args
        .contract
        .as_ref()
        .expect("contract is required for run");
    let function = args
        .function
        .as_ref()
        .expect("function is required for run");

    print_info(format!("Loading contract: {:?}", contract));
    output_writer.write(&format!("Loading contract: {:?}", contract))?;
    logging::log_loading_contract(&contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err((crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                wasm_hash.clone(),
            ))
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
        logging::log_repeat_execution(function, n as usize);
        let runner = RepeatRunner::new(wasm_bytes, args.breakpoint, initial_storage);
        let stats = runner.run(function, parsed_args.as_deref(), n)?;
        stats.display();
        return Ok(());
    }

    print_info("\nStarting debugger...");
    output_writer.write("Starting debugger...")?;
    print_info(format!("Function: {}", function));
    output_writer.write(&format!("Function: {}", function))?;
    if let Some(ref parsed) = parsed_args {
        print_info(format!("Arguments: {}", parsed));
        output_writer.write(&format!("Arguments: {}", parsed))?;
    }
    logging::log_execution_start(function, parsed_args.as_deref());

    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;
    executor.set_timeout(args.timeout);

    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }
    if !args.mock.is_empty() {
        executor.set_mock_specs(&args.mock)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone());

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
            run_instruction_stepping(&mut engine, function, parsed_args.as_deref())?;
            return Ok(());
        }
    }

    print_info("\n--- Execution Start ---\n");
    output_writer.write("\n--- Execution Start ---\n")?;
    let storage_before = engine.executor().get_storage_snapshot()?;
    let result = engine.execute(function, parsed_args.as_deref())?;
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
            let test_code = crate::codegen::TestGenerator::generate(record, contract)?;
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
            contract_hash: contract.to_string_lossy().to_string(),
            function: function.clone(),
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
        let inspector =
            crate::inspector::storage::StorageInspector::with_state(storage_after.clone());
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
                #[allow(clippy::clone_on_copy)]
                let mut footprint_map = std::collections::HashMap::new();
                for (k, v) in &footprint.0 {
                    #[allow(clippy::clone_on_copy)]
                    footprint_map.insert(k.clone(), v.clone());
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

    if args.is_json_output() {
        let mut result_obj = serde_json::json!({
            "result": result,
            "sha256": wasm_hash,
            "budget": {
                "cpu_instructions": budget.cpu_instructions,
                "memory_bytes": budget.memory_bytes,
            },
            "storage_diff": storage_diff,
        });

        if let Some(ref events) = json_events {
            result_obj["events"] = EventInspector::to_json_value(events);
        }
        if let Some(auth_tree) = json_auth {
            result_obj["auth"] = crate::inspector::auth::AuthInspector::to_json_value(&auth_tree);
        }
        if !mock_calls.is_empty() {
            result_obj["mock_calls"] = serde_json::Value::Array(
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
            result_obj["ledger_entries"] = ledger.to_json();
        }

        let output = crate::output::VersionedOutput::success("run", result_obj);

        match serde_json::to_string_pretty(&output) {
            Ok(json) => println!("{}", json),
            Err(e) => {
                let err_output = crate::output::VersionedOutput::<serde_json::Value>::error(
                    "run",
                    format!("Failed to serialize output: {}", e),
                );
                if let Ok(err_json) = serde_json::to_string_pretty(&err_output) {
                    println!("{}", err_json);
                }
            }
        }
    }

    if let Some(trace_path) = &args.trace_output {
        print_info(format!("\nExporting execution trace to: {:?}", trace_path));

        let args_str = parsed_args
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());

        let trace_events = json_events
            .clone()
            .unwrap_or_else(|| engine.executor().get_events().unwrap_or_default());

        let trace = build_execution_trace(
            function,
            contract.to_string_lossy().as_ref(),
            args_str,
            &storage_after,
            &result,
            budget.clone(),
            engine.executor(),
            &trace_events,
            usize::MAX,
        );

        if let Ok(json) = trace.to_json() {
            if let Err(e) = std::fs::write(trace_path, json) {
                print_warning(format!("Failed to write trace to {:?}: {}", trace_path, e));
            } else {
                print_success(format!("Successfully exported trace to {:?}", trace_path));
                if let Err(e) =
                    export_replay_artifact_manifest(&trace, trace_path, contract.as_ref(), &args)
                {
                    print_warning(format!(
                        "Failed to write replay artifact manifest for {:?}: {}",
                        trace_path, e
                    ));
                }
            }
        }
    }

    if let Some(timeline_path) = &args.timeline_output {
        print_info(format!(
            "\nExporting timeline narrative to: {:?}",
            timeline_path
        ));

        let stack_summary = engine
            .state()
            .lock()
            .ok()
            .map(|state| state.call_stack().get_stack().to_vec())
            .unwrap_or_default();

        let mut warnings = Vec::new();
        if !storage_diff.triggered_alerts.is_empty() {
            warnings.push(TimelineWarning {
                kind: "storage_alert".to_string(),
                message: format!(
                    "Triggered storage alert(s): {}",
                    storage_diff.triggered_alerts.join(", ")
                ),
            });
        }

        let events_count = json_events
            .as_ref()
            .map(|ev| ev.len())
            .or_else(|| engine.executor().get_events().ok().map(|ev| ev.len()));

        let storage_delta = if storage_diff.is_empty() {
            None
        } else {
            Some(TimelineStorageDelta::from_storage_diff(&storage_diff, 200))
        };

        let mut pauses = Vec::new();
        let hit_entry_breakpoint = args.breakpoint.iter().any(|bp| bp == function);
        if engine.is_paused() && hit_entry_breakpoint {
            pauses.push(TimelinePausePoint {
                index: 0,
                reason: "breakpoint".to_string(),
                location: None,
                call_stack: stack_summary.clone(),
            });
        }

        let export = TimelineExport {
            schema_version: TIMELINE_EXPORT_SCHEMA_VERSION,
            created_at: chrono::Utc::now().to_rfc3339(),
            run: TimelineRunInfo {
                contract_path: contract.to_string_lossy().to_string(),
                wasm_sha256: Some(wasm_hash.clone()),
                function: function.to_string(),
                args_json: args.args.clone(),
                result: Some(result.clone()),
                error: None,
                budget: Some(budget.clone()),
                events_count,
            },
            pauses,
            stack_summary,
            deltas: TimelineDeltas {
                storage: storage_delta,
            },
            warnings,
        };

        if let Err(e) = write_json_pretty_file(timeline_path, &export) {
            print_warning(format!(
                "Failed to write timeline narrative to {:?}: {}",
                timeline_path, e
            ));
        } else {
            print_success(format!(
                "Successfully exported timeline narrative to {:?}",
                timeline_path
            ));
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
        schema_version: Some(crate::output::SCHEMA_VERSION.to_string()),
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

fn export_replay_artifact_manifest(
    trace: &crate::compare::ExecutionTrace,
    trace_path: &std::path::Path,
    contract_path: &std::path::Path,
    args: &RunArgs,
) -> Result<()> {
    let manifest_path = crate::compare::ExecutionTrace::manifest_path_for_trace(trace_path);
    let mut manifest = trace.to_replay_artifact_manifest(trace_path);

    manifest.files.push(crate::output::ReplayArtifactFile {
        kind: crate::output::ReplayArtifactKind::Manifest,
        path: manifest_path.display().to_string(),
        description: Some("Replay artifact manifest".to_string()),
        compression: None,
    });
    manifest.files.push(crate::output::ReplayArtifactFile {
        kind: crate::output::ReplayArtifactKind::ContractWasm,
        path: contract_path.display().to_string(),
        description: Some("Contract WASM used to generate the trace".to_string()),
        compression: None,
    });

    if let Some(path) = &args.network_snapshot {
        manifest.files.push(crate::output::ReplayArtifactFile {
            kind: crate::output::ReplayArtifactKind::NetworkSnapshot,
            path: path.display().to_string(),
            description: Some("Network snapshot loaded before execution".to_string()),
            compression: None,
        });
    }
    if let Some(path) = &args.import_storage {
        manifest.files.push(crate::output::ReplayArtifactFile {
            kind: crate::output::ReplayArtifactKind::StorageImport,
            path: path.display().to_string(),
            description: Some("Imported storage seed used before execution".to_string()),
            compression: None,
        });
    }
    if let Some(path) = &args.export_storage {
        manifest.files.push(crate::output::ReplayArtifactFile {
            kind: crate::output::ReplayArtifactKind::StorageExport,
            path: path.display().to_string(),
            description: Some("Exported storage state captured after execution".to_string()),
            compression: None,
        });
    }
    if let Some(path) = &args.save_output {
        manifest.files.push(crate::output::ReplayArtifactFile {
            kind: crate::output::ReplayArtifactKind::OutputReport,
            path: path.display().to_string(),
            description: Some("Saved command output for this run".to_string()),
            compression: None,
        });
    }
    if let Some(path) = &args.generate_test {
        manifest.files.push(crate::output::ReplayArtifactFile {
            kind: crate::output::ReplayArtifactKind::GeneratedTest,
            path: path.display().to_string(),
            description: Some("Generated reproduction test derived from the trace".to_string()),
            compression: None,
        });
    }

    crate::history::write_json_atomically(&manifest_path, &manifest)?;
    print_success(format!(
        "Replay artifact manifest written to {:?}",
        manifest_path
    ));
    Ok(())
}

/// Execute run command in dry-run mode.
fn run_dry_run(args: &RunArgs) -> Result<()> {
    let contract = args
        .contract
        .as_ref()
        .expect("contract is required for dry-run");
    print_info(format!("[DRY RUN] Loading contract: {:?}", contract));

    let wasm_file = crate::utils::wasm::load_wasm(contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err((crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                wasm_hash.clone(),
            ))
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
                ((*count as f64) / (counts.total as f64)) * 100.0
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
    print_info(format!("Loading old contract: {:?}", args.old));
    let old_wasm = fs::read(&args.old)
        .map_err(|e| miette::miette!("Failed to read old WASM file {:?}: {}", args.old, e))?;

    print_info(format!("Loading new contract: {:?}", args.new));
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
        "json" => {
            let envelope = crate::output::VersionedOutput::success("upgrade-check", &report);
            serde_json::to_string_pretty(&envelope)
                .map_err(|e| miette::miette!("Failed to serialize report: {}", e))?
        }
        _ => format_text_report(&report),
    };

    if let Some(out_file) = &args.output_file {
        fs::write(out_file, &output)
            .map_err(|e| miette::miette!("Failed to write report to {:?}: {}", out_file, e))?;
        print_success(format!("Report written to {:?}", out_file));
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

/// Run test inputs against both WASM versions and collect diffs
fn run_test_inputs(
    inputs_json: &str,
    old_wasm: &[u8],
    new_wasm: &[u8],
) -> Result<Vec<ExecutionDiff>> {
    let inputs: serde_json::Map<String, serde_json::Value> = serde_json
        ::from_str(inputs_json)
        .map_err(|e|
            miette::miette!(
                "Invalid --test-inputs JSON (expected an object mapping function names to arg arrays): {}",
                e
            )
        )?;

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
    out.push_str(&format!(
        "Status: {} (Classification: {})\n",
        status, report.classification
    ));

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
            return Err((crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                wasm_hash.clone(),
            ))
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

    let rendered = match args.format {
        OutputFormat::Pretty => optimizer.generate_markdown_report(&report),
        OutputFormat::Json => render_optimize_report_json(&report)?,
    };

    if let Some(output_path) = &args.output {
        fs::write(output_path, &rendered).map_err(|e| {
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
        logging::log_display(&rendered, logging::LogLevel::Info);
    }

    Ok(())
}

/// Serialize an `OptimizationReport` to a stable JSON document for the
/// `optimize --format json` flag. The shape is intentionally flat and uses
/// strings (not enum discriminants) for `priority` and `category` so external
/// tools — dashboards, CI gates, IDE plugins — can consume it without needing
/// to track internal enum names.
fn render_optimize_report_json(
    report: &crate::profiler::analyzer::OptimizationReport,
) -> Result<String> {
    let suggestions: Vec<serde_json::Value> = report
        .suggestions
        .iter()
        .map(|s| {
            serde_json::json!({
                "category": s.category,
                "title": s.title,
                "description": s.description,
                "estimated_cpu_savings": s.estimated_cpu_savings,
                "estimated_memory_savings": s.estimated_memory_savings,
                "location": s.location,
                "priority": s.priority.to_string(),
            })
        })
        .collect();

    let hotspots: Vec<serde_json::Value> = report
        .functions
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name,
                "cpu_instructions": f.total_cpu,
                "memory_bytes": f.total_memory,
                "wall_time_ms": f.wall_time_ms as u64,
            })
        })
        .collect();

    let doc = serde_json::json!({
        "schema_version": crate::output::SCHEMA_VERSION,
        "kind": "optimize_report",
        "contract": report.contract_path,
        "metadata": {
            "total_cpu_instructions": report.total_cpu,
            "total_memory_bytes": report.total_memory,
            "potential_cpu_savings": report.potential_cpu_savings,
            "potential_memory_savings": report.potential_memory_savings,
            "suggestion_count": report.suggestions.len(),
            "function_count": report.functions.len(),
        },
        "hotspots": hotspots,
        "suggestions": suggestions,
    });

    serde_json::to_string_pretty(&doc).map_err(|e| {
        DebuggerError::FileError(format!(
            "Failed to serialize optimize report as JSON: {}",
            e
        ))
        .into()
    })
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
            return Err((crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                wasm_hash.clone(),
            ))
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

    // Format output based on export_format
    let output_content = match args.export_format {
        crate::cli::args::ProfileExportFormat::FoldedStack => {
            // Export in folded stack format for external tools (issue #502)
            optimizer.to_folded_stack_format(&report)
        }
        crate::cli::args::ProfileExportFormat::Json => {
            // Export as JSON with basic metrics
            let func_names: Vec<String> = report.functions.iter().map(|f| f.name.clone()).collect();
            serde_json::to_string_pretty(&serde_json::json!({
                "contract": contract_path_str,
                "functions": func_names,
                "total_cpu": report.total_cpu,
                "total_memory": report.total_memory,
                "potential_cpu_savings": report.potential_cpu_savings,
                "potential_memory_savings": report.potential_memory_savings,
            }))
            .unwrap_or_else(|_| "{}".to_string())
        }
        crate::cli::args::ProfileExportFormat::Report => {
            // Default markdown report
            let hotspots = report.format_hotspots();
            let markdown = optimizer.generate_markdown_report(&report);
            logging::log_display(format!("\n{}", hotspots), logging::LogLevel::Info);
            markdown
        }
    };

    if let Some(output_path) = &args.output {
        fs::write(output_path, &output_content).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write report to {:?}: {}",
                output_path, e
            ))
        })?;
        logging::log_display(
            format!("\nProfile report written to: {:?}", output_path),
            logging::LogLevel::Info,
        );
    } else if !matches!(
        args.export_format,
        crate::cli::args::ProfileExportFormat::Report
    ) {
        // Only print output_content for non-Report formats if no file specified
        logging::log_display(format!("\n{}", output_content), logging::LogLevel::Info);
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
    let filters = crate::compare::engine::CompareFilters::new(
        args.ignore_path.clone(),
        args.ignore_field.clone(),
    )?;
    let report = crate::compare::CompareEngine::compare_with_filters(&trace_a, &trace_b, &filters);
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
        println!("{}", rendered);
    }

    Ok(())
}

/// Execute the replay command.
/// Execute the replay command.
pub fn replay(args: ReplayArgs, verbosity: Verbosity) -> Result<()> {
    print_info(format!("Loading trace file: {:?}", args.trace_file));
    let original_trace = crate::compare::ExecutionTrace::from_file(&args.trace_file)?;

    // Fail fast on malformed or unsupported-schema traces before touching the
    // executor. validate_for_replay knows whether --contract covers a missing
    // trace.contract, so it does the contract-existence check for us.
    original_trace.validate_for_replay(args.contract.as_deref())?;

    if let Some(version) = original_trace.schema_version.as_deref() {
        print_info(format!("Trace schema version: {}", version));
    }

    // Determine which contract to use (validate_for_replay guarantees at
    // least one of the two is set).
    let contract_path = if let Some(path) = &args.contract {
        path.clone()
    } else {
        std::path::PathBuf::from(
            original_trace
                .contract
                .as_deref()
                .expect("validate_for_replay guarantees a contract source"),
        )
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
    print_info(format!(
        "Starting remote debug server on {}:{}",
        args.host, args.port
    ));
    if let Some(token) = &args.token {
        print_info("Token authentication enabled");
        if token.trim().len() < 16 {
            print_warning(
                "Remote debug token is shorter than 16 characters. Prefer at least 16 characters \
                 and ideally a random 32-byte token.",
            );
        }
    } else {
        print_info("Token authentication disabled");
    }
    if args.tls_cert.is_some() || args.tls_key.is_some() {
        print_info("TLS enabled");
    } else if args.token.is_some() {
        print_warning(
            "Token authentication is enabled without TLS. Assume traffic is plaintext unless you \
             are using a trusted private network or external TLS termination.",
        );
    }

    let server = crate::server::DebugServer::new(
        args.host.clone(),
        args.token.clone(),
        args.tls_cert.as_deref(),
        args.tls_key.as_deref(),
        args.repeat,
        args.storage_filter,
        args.show_events,
        args.event_filter,
        args.mock,
    )?;

    tokio::runtime::Runtime::new()
        .map_err(|e: std::io::Error| miette::miette!(e))
        .and_then(|rt| rt.block_on(server.run(args.port)))
}

/// Connect to remote debug server
pub fn remote(args: RemoteArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Connecting to remote debugger at {}", args.remote));

    // Build per-request timeouts, falling back to the general --timeout-ms for
    // the specialised classes when the user did not set them explicitly.
    let default_ms = args.timeout_ms;
    let timeouts = crate::client::RemoteClientConfig::build_timeouts(
        default_ms,
        args.inspect_timeout_ms,
        args.storage_timeout_ms,
    );

    let config = crate::client::RemoteClientConfig {
        connect_timeout: std::time::Duration::from_millis(args.connect_timeout_ms),
        timeouts,
        retry: crate::client::RetryPolicy {
            max_attempts: args.retry_attempts,
            base_delay: std::time::Duration::from_millis(args.retry_base_delay_ms),
            max_delay: std::time::Duration::from_millis(args.retry_max_delay_ms),
        },
        tls_cert: args.tls_cert.clone(),
        tls_key: args.tls_key.clone(),
        tls_ca: args.tls_ca.clone(),
        session_label: args.session_label.clone(),
        ..Default::default()
    };

    let mut client =
        crate::client::RemoteClient::connect_with_config(&args.remote, args.token.clone(), config).map_err(|e| {
            // Enrich connect-specific errors with a hint about --connect-timeout-ms so
            // the user knows which knob to turn without having to read the docs first.
            let msg = e.to_string();
            if msg.contains("Request timed out") || msg.contains("timed out") || msg.contains("Connection refused") || msg.contains("Network/transport error") {
                miette::miette!("{}\n\nHint: use --connect-timeout-ms <MS> (current: {}ms) to extend the initial TCP connect window, or set SOROBAN_DEBUG_CONNECT_TIMEOUT_MS. See docs/remote-troubleshooting.md for the full diagnostic matrix.",
                    msg,
                    args.connect_timeout_ms)
            } else {
                miette::miette!("{}", msg)
            }
        })?;

    if let Some(info) = client.session_info() {
        print_info(format!(
            "Remote session: {} (created {}, label={})",
            info.session_id,
            info.created_at,
            info.label.as_deref().unwrap_or("<none>")
        ));
    }

    if let Some(contract) = &args.contract {
        print_info(format!("Loading contract: {:?}", contract));
        let size = client.load_contract(&contract.to_string_lossy())?;
        print_success(format!("Contract loaded: {} bytes", size));
    }

    if let Some(action) = &args.action {
        return match action {
            RemoteAction::Inspect => {
                let (function, step_count, paused, call_stack, pause_reason) = client.inspect()?;
                println!("Function: {}", function.as_deref().unwrap_or("<none>"));
                println!("Step count: {}", step_count);
                println!("Paused: {}", paused);
                if let Some(reason) = pause_reason {
                    println!("Pause reason: {}", reason);
                }
                if !call_stack.is_empty() {
                    println!("Call stack:");
                    for frame in &call_stack {
                        println!("  {}", frame);
                    }
                }
                Ok(())
            }
            RemoteAction::Storage => {
                let storage_json = client.get_storage()?;
                println!("{}", storage_json);
                Ok(())
            }
            RemoteAction::Evaluate(eval_args) => {
                let (result, result_type) =
                    client.evaluate(&eval_args.expression, eval_args.frame_id)?;
                if let Some(rtype) = &result_type {
                    println!("[{}] {}", rtype, result);
                } else {
                    println!("{}", result);
                }
                Ok(())
            }
        };
    }

    if let Some(function) = &args.function {
        print_info(format!("Executing function: {}", function));
        let result = client.execute(function, args.args.as_deref())?;
        print_success(format!("Result: {}", result));
        return Ok(());
    }

    client.ping()?;
    print_success("Remote debugger is reachable");
    Ok(())
}
/// Launch interactive debugger UI
pub fn interactive(args: InteractiveArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    logging::log_loading_contract(&args.contract.to_string_lossy());

    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;
    let wasm_hash = wasm_file.sha256_hash;

    if let Some(expected) = &args.expected_hash {
        if expected.to_lowercase() != wasm_hash {
            return Err((crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                wasm_hash.clone(),
            ))
            .into());
        }
    }

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("Loading network snapshot: {:?}", snapshot_path));
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

    let mut initial_storage = if let Some(storage_json) = &args.storage {
        Some(parse_storage(storage_json)?)
    } else {
        None
    };

    if let Some(import_path) = &args.import_storage {
        print_info(format!("Importing storage from: {:?}", import_path));
        let imported = crate::inspector::storage::StorageState::import_from_file(import_path)?;
        print_success(format!("Imported {} storage entries", imported.len()));
        initial_storage = Some(serde_json::to_string(&imported).map_err(|e| {
            DebuggerError::StorageError(format!("Failed to serialize imported storage: {}", e))
        })?);
    }

    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;
    executor.set_timeout(args.timeout);

    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }
    if !args.mock.is_empty() {
        executor.set_mock_specs(&args.mock)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone());

    if args.instruction_debug {
        print_info("Enabling instruction-level debugging...");
        engine.enable_instruction_debug(&wasm_bytes)?;

        if args.step_instructions {
            let step_mode = parse_step_mode(&args.step_mode);
            engine.start_instruction_stepping(step_mode)?;
        }
    }

    print_info("Starting interactive session (type 'help' for commands)");
    let mut ui = DebuggerUI::new(engine)?;
    ui.queue_execution(args.function.clone(), parsed_args);
    ui.run()
}

/// Launch TUI debugger
pub fn tui(args: TuiArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    let wasm_bytes = wasm_file.bytes;

    print_success(format!(
        "Contract loaded successfully ({} bytes)",
        wasm_bytes.len()
    ));

    if let Some(snapshot_path) = &args.network_snapshot {
        print_info(format!("Loading network snapshot: {:?}", snapshot_path));
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

    let mut executor = ContractExecutor::new(wasm_bytes.clone())?;

    if let Some(storage) = initial_storage {
        executor.set_initial_storage(storage)?;
    }

    let mut engine = DebuggerEngine::new(executor, args.breakpoint.clone());
    engine.stage_execution(&args.function, parsed_args.as_deref());

    run_dashboard(engine, &args.function)
}

/// Inspect a WASM contract
pub fn inspect(args: InspectArgs, _verbosity: Verbosity) -> Result<()> {
    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    if let Some(expected) = &args.expected_hash {
        if !wasm_file.sha256_hash.eq_ignore_ascii_case(expected) {
            return Err(crate::DebuggerError::ChecksumMismatch(
                expected.clone(),
                wasm_file.sha256_hash.clone(),
            )
            .into());
        }
    }

    let bytes = wasm_file.bytes;

    if args.source_map_diagnostics {
        return inspect_source_map_diagnostics(&args, &bytes);
    }

    let info = crate::utils::wasm::get_module_info(&bytes)?;
    let artifact_metadata = crate::utils::wasm::extract_wasm_artifact_metadata(&bytes)?;
    if args.format == OutputFormat::Json {
        let exported_functions = if args.functions {
            Some(crate::utils::wasm::parse_function_signatures(&bytes)?)
        } else {
            None
        };
        let result = serde_json::json!({
            "contract": args.contract.display().to_string(),
            "size_bytes": info.total_size,
            "types": info.type_count,
            "functions": info.function_count,
            "exports": info.export_count,
            "exported_functions": exported_functions,
            "artifact_metadata": artifact_metadata,
        });
        let envelope = crate::output::VersionedOutput::success("inspect", result);
        println!(
            "{}",
            serde_json::to_string_pretty(&envelope).map_err(|e| {
                DebuggerError::FileError(format!("Failed to serialize inspect JSON output: {}", e))
            })?
        );
        return Ok(());
    }

    println!("Contract: {:?}", args.contract);
    println!("Size: {} bytes", info.total_size);
    println!("Types: {}", info.type_count);
    println!("Functions: {}", info.function_count);
    println!("Exports: {}", info.export_count);
    println!("Artifact metadata:");
    println!(
        "  Build profile hint: {}",
        artifact_metadata.build_profile_hint
    );
    println!(
        "  Optimization hint: {}",
        artifact_metadata.optimization_hint
    );
    println!(
        "  Name section: {}",
        if artifact_metadata.name_section_present {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  DWARF debug sections: {}",
        if artifact_metadata.has_debug_sections {
            if artifact_metadata.debug_sections.is_empty() {
                "present".to_string()
            } else {
                format!(
                    "present ({}, {} bytes)",
                    artifact_metadata.debug_sections.join(", "),
                    artifact_metadata.debug_section_bytes
                )
            }
        } else {
            "absent".to_string()
        }
    );
    if let Some(module_name) = &artifact_metadata.module_name {
        println!("  Module name: {}", module_name);
    }
    if !artifact_metadata.package_hints.is_empty() {
        println!("  Package hints:");
        for hint in &artifact_metadata.package_hints {
            println!("    - {}", hint);
        }
    }
    if !artifact_metadata.producers.is_empty() {
        println!("  Producers:");
        for field in &artifact_metadata.producers {
            let values = field
                .values
                .iter()
                .map(|value| {
                    if value.version.is_empty() {
                        value.name.clone()
                    } else {
                        format!("{} {}", value.name, value.version)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            println!("    {}: {}", field.name, values);
        }
    }
    if !artifact_metadata.heuristic_notes.is_empty() {
        println!("  Notes:");
        for note in &artifact_metadata.heuristic_notes {
            println!("    - {}", note);
        }
    }
    if args.functions {
        let sigs = crate::utils::wasm::parse_function_signatures(&bytes)?;
        println!("Exported functions:");
        for sig in &sigs {
            let params: Vec<String> = sig
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.type_name))
                .collect();
            let ret = sig.return_type.as_deref().unwrap_or("()");
            println!("  {}({}) -> {}", sig.name, params.join(", "), ret);
        }
    }
    Ok(())
}

fn inspect_source_map_diagnostics(args: &InspectArgs, wasm_bytes: &[u8]) -> Result<()> {
    let report =
        crate::debugger::source_map::SourceMap::inspect_wasm(wasm_bytes, args.source_map_limit)?;

    match args.format {
        OutputFormat::Json => {
            let output = SourceMapDiagnosticsCommandOutput {
                contract: args.contract.display().to_string(),
                source_map: report,
            };
            let pretty = serde_json::to_string_pretty(&output).map_err(|e| {
                DebuggerError::ExecutionError(format!(
                    "Failed to serialize source-map diagnostics JSON output: {e}"
                ))
            })?;
            println!("{pretty}");
        }
        OutputFormat::Pretty => {
            println!("Source Map Diagnostics");
            println!("Contract: {}", args.contract.display());
            println!("Resolved mappings: {}", report.mappings_count);
            println!("Fallback mode: {}", report.fallback_mode);
            println!("Fallback behavior: {}", report.fallback_message);

            println!("\nDWARF sections:");
            for section in &report.sections {
                let status = if section.present {
                    "present"
                } else {
                    "missing"
                };
                println!(
                    "  {}: {} ({} bytes)",
                    section.name, status, section.size_bytes
                );
            }

            if report.preview.is_empty() {
                println!("\nResolved mappings preview: none");
            } else {
                println!("\nResolved mappings preview:");
                for mapping in &report.preview {
                    let column = mapping
                        .location
                        .column
                        .map(|column| format!(":{}", column))
                        .unwrap_or_default();
                    println!(
                        "  0x{offset:08x} -> {file}:{line}{column}",
                        offset = mapping.offset,
                        file = mapping.location.file.display(),
                        line = mapping.location.line,
                        column = column
                    );
                }
            }

            if report.diagnostics.is_empty() {
                println!("\nDiagnostics: none");
            } else {
                println!("\nDiagnostics:");
                for diagnostic in &report.diagnostics {
                    println!("  - {}", diagnostic.message);
                }
            }
        }
    }

    Ok(())
}

/// Run symbolic execution analysis
pub fn symbolic(args: SymbolicArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    let analyzer = SymbolicAnalyzer::new();
    let config = symbolic_config_from_args(&args)?;
    let report = analyzer.analyze_with_config(&wasm_file.bytes, &args.function, &config)?;

    match args.format {
        OutputFormat::Pretty => {
            println!("{}", render_symbolic_report(&report));
        }
        OutputFormat::Json => {
            let envelope = crate::output::VersionedOutput::success("symbolic", &report);
            println!(
                "{}",
                serde_json::to_string_pretty(&envelope).map_err(|e| {
                    DebuggerError::FileError(format!("Failed to serialize symbolic report: {}", e))
                })?
            );
        }
    }

    if let Some(output_path) = &args.output {
        let scenario_toml = analyzer.generate_scenario_toml(&report);
        fs::write(output_path, scenario_toml).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write symbolic scenario to {:?}: {}",
                output_path, e
            ))
        })?;
        print_success(format!("Scenario TOML written to: {:?}", output_path));
    }

    if let Some(bundle_path) = &args.export_replay_bundle {
        let bundle = build_replay_bundle(
            &config,
            &report,
            wasm_file.sha256_hash.clone(),
            Some(args.contract.to_string_lossy().to_string()),
        );
        let serialized = serde_json::to_string_pretty(&bundle).map_err(|e| {
            DebuggerError::FileError(format!("Failed to serialize replay bundle to JSON: {}", e))
        })?;
        fs::write(bundle_path, serialized).map_err(|e| {
            DebuggerError::FileError(format!(
                "Failed to write replay bundle to {:?}: {}",
                bundle_path, e
            ))
        })?;
        print_success(format!("Replay bundle written to: {:?}", bundle_path));
    }

    Ok(())
}

/// Analyze a contract
pub fn analyze(args: AnalyzeArgs, _verbosity: Verbosity) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;

    let mut dynamic_analysis = None;
    let mut warnings = Vec::new();
    let mut executor = None;
    let mut trace_entries = None;

    if let Some(function) = &args.function {
        let mut dynamic_executor = ContractExecutor::new(wasm_file.bytes.clone())?;
        dynamic_executor.enable_mock_all_auths();
        dynamic_executor.set_timeout(args.timeout);

        if let Some(storage_json) = &args.storage {
            dynamic_executor.set_initial_storage(parse_storage(storage_json)?)?;
        }

        let parsed_args = if let Some(args_json) = &args.args {
            Some(parse_args(args_json)?)
        } else {
            None
        };

        match dynamic_executor.execute(function, parsed_args.as_deref()) {
            Ok(result) => {
                let trace = dynamic_executor.get_dynamic_trace().unwrap_or_default();

                dynamic_analysis = Some(DynamicAnalysisMetadata {
                    function: function.clone(),
                    args: parsed_args.clone(),
                    result: Some(result),
                    trace_entries: trace.len(),
                });
                trace_entries = Some(trace);
                executor = Some(dynamic_executor);
            }
            Err(err) => {
                warnings.push(format!(
                    "Dynamic analysis for function '{}' failed: {}",
                    function, err
                ));
            }
        }
    }

    let mut analyzer = SecurityAnalyzer::new();
    let config = crate::config::Config::load_or_default();
    if let Some(supp_path) = config.output.suppressions_file {
        if std::path::Path::new(&supp_path).exists() {
            analyzer = analyzer.load_suppressions_from_file(&supp_path)?;
        }
    }
    let filter = crate::analyzer::security::AnalyzerFilter {
        enable_rules: args.enable_rule.clone(),
        disable_rules: args.disable_rule.clone(),
        min_severity: parse_min_severity(&args.min_severity)?,
    };
    let contract_path = args.contract.to_string_lossy().to_string();
    let report = analyzer.analyze(
        &wasm_file.bytes,
        executor.as_ref(),
        trace_entries.as_deref(),
        &filter,
        &contract_path,
    )?;
    let output = AnalyzeCommandOutput {
        findings: report.findings,
        dynamic_analysis,
        warnings,
        suppressed_count: report.metadata.suppressed_count,
    };

    match args.format.to_lowercase().as_str() {
        "text" => println!("{}", render_security_report(&output)),
        "json" => {
            let envelope = crate::output::VersionedOutput::success("analyze", &output);
            println!(
                "{}",
                serde_json::to_string_pretty(&envelope).map_err(|e| {
                    DebuggerError::FileError(format!("Failed to serialize analysis output: {}", e))
                })?
            );
        }
        other => {
            return Err(DebuggerError::InvalidArguments(format!(
                "Unsupported --format '{}'. Use 'text' or 'json'.",
                other
            ))
            .into());
        }
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct DoctorCheck {
    ok: bool,
    message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct RemoteDoctorReport {
    address: String,
    connect: DoctorCheck,
    handshake: Option<DoctorCheck>,
    ping: Option<DoctorCheck>,
    auth: Option<DoctorCheck>,
    selected_protocol: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DoctorReport {
    binary: serde_json::Value,
    config: serde_json::Value,
    history: serde_json::Value,
    plugins: serde_json::Value,
    protocol: serde_json::Value,
    remote: Option<RemoteDoctorReport>,
    vscode_extension: serde_json::Value,
}

fn check_ok(message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        ok: true,
        message: message.into(),
    }
}

fn check_err(message: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        ok: false,
        message: message.into(),
    }
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn read_repo_vscode_extension_version(manifest_path: Option<&PathBuf>) -> Option<String> {
    let path = manifest_path.cloned().unwrap_or_else(|| {
        PathBuf::from("extensions")
            .join("vscode")
            .join("package.json")
    });
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("version")?.as_str().map(|s| s.to_string())
}

fn compute_default_history_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("SOROBAN_DEBUG_HISTORY_FILE") {
        return Ok(PathBuf::from(path));
    }

    let home_dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| DebuggerError::FileError("Could not determine home directory".to_string()))?;
    Ok(PathBuf::from(home_dir)
        .join(".soroban-debug")
        .join("history.json"))
}

fn history_file_status(path: &PathBuf) -> serde_json::Value {
    let exists = path.exists();
    let metadata = std::fs::metadata(path).ok();
    let size = metadata.as_ref().map(|m| m.len());

    let readable = std::fs::File::open(path).is_ok();
    let writable = std::fs::OpenOptions::new().append(true).open(path).is_ok();

    serde_json::json!({
        "path": path,
        "exists": exists,
        "size_bytes": size,
        "readable": readable || !exists,
        "writable": writable || !exists,
    })
}

fn config_status() -> serde_json::Value {
    let path = std::path::Path::new(crate::config::DEFAULT_CONFIG_FILE).to_path_buf();
    let exists = path.exists();
    let load = crate::config::Config::load();
    let parse_ok = load.is_ok() || !exists;
    let error = load.err().map(|e| e.to_string());

    serde_json::json!({
        "path": path,
        "exists": exists,
        "parse_ok": parse_ok,
        "error": error,
    })
}

fn plugin_status() -> serde_json::Value {
    let disabled = env_truthy("SOROBAN_DEBUG_NO_PLUGINS");
    let plugin_dir = crate::plugin::PluginLoader::default_plugin_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());

    let discovered = crate::plugin::PluginLoader::default_plugin_dir()
        .map(|dir| crate::plugin::PluginLoader::new(dir).discover_plugins())
        .unwrap_or_default();

    let registry = crate::plugin::registry::init_global_plugin_registry();
    let stats = registry.read().map(|r| r.statistics()).unwrap_or_default();

    serde_json::json!({
        "disabled_via_env": disabled,
        "plugin_dir": plugin_dir,
        "discovered_manifests": discovered.len(),
        "loaded_plugins": stats.total,
        "provides_commands": stats.provides_commands,
        "provides_formatters": stats.provides_formatters,
        "supports_hot_reload": stats.supports_hot_reload,
    })
}

fn protocol_status() -> serde_json::Value {
    serde_json::json!({
        "min": crate::server::protocol::PROTOCOL_MIN_VERSION,
        "max": crate::server::protocol::PROTOCOL_MAX_VERSION,
        "current": crate::server::protocol::PROTOCOL_VERSION,
    })
}

fn binary_status() -> serde_json::Value {
    serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

fn vscode_extension_status(vscode_manifest: Option<&PathBuf>) -> serde_json::Value {
    let version = read_repo_vscode_extension_version(vscode_manifest);
    serde_json::json!({
        "version_hint": version,
        "wire_protocol_expected_min": crate::server::protocol::PROTOCOL_MIN_VERSION,
        "wire_protocol_expected_max": crate::server::protocol::PROTOCOL_MAX_VERSION,
    })
}

/// Run a scenario
pub fn scenario(args: ScenarioArgs, _verbosity: Verbosity) -> Result<()> {
    crate::scenario::run_scenario(args, _verbosity)
}

/// Launch the REPL
pub async fn repl(args: ReplArgs) -> Result<()> {
    print_info(format!("Loading contract: {:?}", args.contract));
    let wasm_file = crate::utils::wasm::load_wasm(&args.contract)
        .with_context(|| format!("Failed to read WASM file: {:?}", args.contract))?;
    crate::utils::wasm::verify_wasm_hash(&wasm_file.sha256_hash, args.expected_hash.as_ref())?;

    if args.expected_hash.is_some() {
        print_verbose("Checksum verified ✓");
    }

    crate::repl::start_repl(ReplConfig {
        contract_path: args.contract,
        network_snapshot: args.network_snapshot,
        storage: args.storage,
        watch_keys: args.watch_keys,
    })
    .await
}

/// Show budget trend chart
pub fn show_budget_trend(
    contract: Option<&str>,
    function: Option<&str>,
    regression: crate::history::RegressionConfig,
) -> Result<()> {
    let manager = HistoryManager::new()?;
    let mut records = manager.filter_history(contract, function)?;

    crate::history::sort_records_by_date(&mut records);

    if records.is_empty() {
        if !Formatter::is_quiet() {
            println!("Budget Trend");
            println!(
                "Filters: contract={} function={}",
                contract.unwrap_or("*"),
                function.unwrap_or("*")
            );
            println!("No run history found yet.");
            println!("Tip: run `soroban-debug run ...` a few times to generate history.");
        }
        return Ok(());
    }

    let stats = budget_trend_stats_or_err(&records)?;
    let cpu_values: Vec<u64> = records.iter().map(|r| r.cpu_used).collect();
    let mem_values: Vec<u64> = records.iter().map(|r| r.memory_used).collect();

    if !Formatter::is_quiet() {
        println!("Budget Trend");
        println!(
            "Filters: contract={} function={}",
            contract.unwrap_or("*"),
            function.unwrap_or("*")
        );
        println!(
            "Regression params: threshold>{:.1}% lookback={} smoothing={}",
            regression.threshold_pct, regression.lookback, regression.smoothing_window
        );
        println!(
            "Runs: {}   Range: {} -> {}",
            stats.count, stats.first_date, stats.last_date
        );
        println!(
            "CPU insns: last={}  avg={}  min={}  max={}",
            crate::inspector::budget::BudgetInspector::format_cpu_insns(stats.last_cpu),
            crate::inspector::budget::BudgetInspector::format_cpu_insns(stats.cpu_avg),
            crate::inspector::budget::BudgetInspector::format_cpu_insns(stats.cpu_min),
            crate::inspector::budget::BudgetInspector::format_cpu_insns(stats.cpu_max)
        );
        println!(
            "Mem bytes: last={}  avg={}  min={}  max={}",
            crate::inspector::budget::BudgetInspector::format_memory_bytes(stats.last_mem),
            crate::inspector::budget::BudgetInspector::format_memory_bytes(stats.mem_avg),
            crate::inspector::budget::BudgetInspector::format_memory_bytes(stats.mem_min),
            crate::inspector::budget::BudgetInspector::format_memory_bytes(stats.mem_max)
        );
        println!();
        println!("CPU trend: {}", Formatter::sparkline(&cpu_values, 50));
        println!("MEM trend: {}", Formatter::sparkline(&mem_values, 50));

        if let Some((cpu_reg, mem_reg)) =
            crate::history::check_regression_with_config(&records, &regression)
        {
            if cpu_reg > 0.0 || mem_reg > 0.0 {
                println!();
                println!("Regression warning (latest vs baseline):");
                if cpu_reg > 0.0 {
                    println!("  CPU increased by {:.1}%", cpu_reg);
                }
                if mem_reg > 0.0 {
                    println!("  Memory increased by {:.1}%", mem_reg);
                }
            }
        }
    }

    Ok(())
}

/// Prune run history according to retention policy.
pub fn history_prune(args: HistoryPruneArgs) -> Result<()> {
    let policy = crate::history::RetentionPolicy {
        max_records: args.max_records,
        max_age_days: args.max_age_days,
    };

    if policy.is_empty() {
        if !Formatter::is_quiet() {
            println!("No retention policy specified. Use --max-records and/or --max-age-days.");
        }
        return Ok(());
    }

    let manager = HistoryManager::new()?;

    if args.dry_run {
        let mut records = manager.load_history()?;
        let before = records.len();
        HistoryManager::apply_retention(&mut records, &policy);
        let remaining = records.len();
        let removed = before.saturating_sub(remaining);

        if !Formatter::is_quiet() {
            if removed == 0 {
                println!("[dry-run] Nothing removed ({} records).", remaining);
            } else {
                println!(
                    "[dry-run] Would remove {} record(s). {} record(s) remaining.",
                    removed, remaining
                );
            }
        }
        return Ok(());
    }

    let report = manager.prune_history(&policy)?;
    if !Formatter::is_quiet() {
        if report.removed == 0 {
            println!("Nothing removed ({} records).", report.remaining);
        } else {
            println!(
                "Removed {} record(s). {} record(s) remaining.",
                report.removed, report.remaining
            );
        }
    }
    Ok(())
}

pub fn doctor(args: crate::cli::args::DoctorArgs) -> Result<()> {
    let history_path = compute_default_history_path()
        .unwrap_or_else(|_| std::env::temp_dir().join("soroban-debug-history.json"));

    let remote = if let Some(addr) = args.remote.as_ref() {
        let config = crate::client::remote_client::RemoteClientConfig {
            connect_timeout: std::time::Duration::from_millis(args.timeout_ms),
            timeouts: crate::client::remote_client::RemoteClientConfig::build_timeouts(
                args.timeout_ms,
                Some(args.timeout_ms),
                Some(args.timeout_ms),
            ),
            ..crate::client::remote_client::RemoteClientConfig::default()
        };

        match crate::client::remote_client::RemoteClient::connect_with_config(
            addr,
            args.token.clone(),
            config,
        ) {
            Ok(mut client) => {
                let ping = match client.ping() {
                    Ok(_) => Some(check_ok("Ping succeeded")),
                    Err(e) => Some(check_err(format!("Ping failed: {}", e))),
                };
                Some(RemoteDoctorReport {
                    address: addr.clone(),
                    connect: check_ok(format!("Connected to {}", addr)),
                    handshake: Some(check_ok("Handshake succeeded")),
                    ping,
                    auth: args
                        .token
                        .as_ref()
                        .map(|_| check_ok("Authentication succeeded")),
                    selected_protocol: None,
                })
            }
            Err(e) => Some(RemoteDoctorReport {
                address: addr.clone(),
                connect: check_err(format!("Connection failed: {}", e)),
                handshake: None,
                ping: None,
                auth: None,
                selected_protocol: None,
            }),
        }
    } else {
        None
    };

    let report = DoctorReport {
        binary: binary_status(),
        config: config_status(),
        history: history_file_status(&history_path),
        plugins: plugin_status(),
        protocol: protocol_status(),
        remote,
        vscode_extension: vscode_extension_status(args.vscode_manifest.as_ref()),
    };

    if args.format == OutputFormat::Json {
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| miette::miette!("Failed to serialize doctor report: {}", e))?;
        println!("{}", json);
        return Ok(());
    }

    println!("Binary: {}", report.binary);
    println!("Config: {}", report.config);
    println!("History: {}", report.history);
    println!("Plugins: {}", report.plugins);
    println!("Protocol: {}", report.protocol);
    if let Some(remote) = report.remote {
        println!("Remote connect: {}", remote.connect.message);
        if let Some(handshake) = remote.handshake {
            println!("Remote handshake: {}", handshake.message);
        }
        if let Some(ping) = remote.ping {
            println!("Remote ping: {}", ping.message);
        }
        if let Some(auth) = remote.auth {
            println!("Remote auth: {}", auth.message);
        }
    }
    println!("VS Code extension: {}", report.vscode_extension);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_trend_stats_or_err_returns_error_instead_of_panicking() {
        let empty: Vec<RunHistory> = Vec::new();
        let err = budget_trend_stats_or_err(&empty).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Failed to compute budget trend statistics"));
    }

    #[test]
    fn doctor_report_serializes_with_expected_sections() {
        let history_path = std::env::temp_dir().join("soroban-debug-doctor-history.json");
        let report = DoctorReport {
            binary: binary_status(),
            config: config_status(),
            history: history_file_status(&history_path),
            plugins: plugin_status(),
            protocol: protocol_status(),
            remote: None,
            vscode_extension: vscode_extension_status(None),
        };

        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("binary").is_some());
        assert!(json.get("config").is_some());
        assert!(json.get("history").is_some());
        assert!(json.get("plugins").is_some());
        assert!(json.get("protocol").is_some());
        assert!(json.get("vscode_extension").is_some());
    }

    #[test]
    fn optimize_report_json_shape_is_stable() {
        use crate::profiler::analyzer::{
            FunctionProfile, OptimizationReport, OptimizationSuggestion, Priority,
        };
        use std::collections::HashMap;

        let suggestion = OptimizationSuggestion {
            category: "Expensive Operations".to_string(),
            title: "High CPU usage in function 'transfer'".to_string(),
            description: "Function uses many CPU instructions.".to_string(),
            estimated_cpu_savings: 250,
            estimated_memory_savings: 64,
            location: "transfer".to_string(),
            priority: Priority::High,
        };
        let function = FunctionProfile {
            name: "transfer".to_string(),
            total_cpu: 2_500,
            total_memory: 640,
            wall_time_ms: 7,
            operations: Vec::new(),
            storage_accesses: HashMap::new(),
            call_tree: None,
            timeline: None,
        };
        let report = OptimizationReport {
            contract_path: "fixtures/transfer.wasm".to_string(),
            functions: vec![function],
            suggestions: vec![suggestion],
            total_cpu: 2_500,
            total_memory: 640,
            potential_cpu_savings: 250,
            potential_memory_savings: 64,
        };

        let rendered = render_optimize_report_json(&report).expect("json render");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");

        assert_eq!(value["kind"], "optimize_report");
        assert_eq!(value["contract"], "fixtures/transfer.wasm");
        assert!(value["schema_version"].is_string());
        assert_eq!(value["metadata"]["total_cpu_instructions"], 2_500);
        assert_eq!(value["metadata"]["potential_cpu_savings"], 250);
        assert_eq!(value["metadata"]["suggestion_count"], 1);
        assert_eq!(value["metadata"]["function_count"], 1);

        let hotspots = value["hotspots"].as_array().expect("hotspots is array");
        assert_eq!(hotspots.len(), 1);
        assert_eq!(hotspots[0]["name"], "transfer");
        assert_eq!(hotspots[0]["cpu_instructions"], 2_500);
        assert_eq!(hotspots[0]["wall_time_ms"], 7);

        let suggestions = value["suggestions"]
            .as_array()
            .expect("suggestions is array");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0]["priority"], "High");
        assert_eq!(suggestions[0]["category"], "Expensive Operations");
        assert_eq!(suggestions[0]["estimated_cpu_savings"], 250);
    }
}
//
///////
