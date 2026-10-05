//! Interactive TUI Dashboard for Soroban contract debugging.
//!
//! This module provides a full-screen terminal UI built with ratatui that displays:
//! - Call stack information with function names and context
//! - Storage state with key-value pairs
//! - Real-time CPU and memory budget meters with history
//! - Execution log with timestamped events
//!
//! The dashboard supports keyboard navigation between panes (Tab, arrow keys) and
//! debugger control actions (step, continue, refresh).

use crate::debugger::engine::DebuggerEngine;
use crate::inspector::budget::BudgetInfo;
use crate::inspector::stack::CallFrame;
use crate::inspector::storage::{StorageInspector, StorageQuery};
use crate::{DebuggerError, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use std::{
    collections::{HashSet, VecDeque},
    io,
    time::{Duration, Instant},
};

// ─── Palette ────────────────────────────────────────────────────────────────
const COLOR_BG: Color = Color::Rgb(15, 17, 26);
const COLOR_SURFACE: Color = Color::Rgb(22, 27, 40);
const COLOR_BORDER: Color = Color::Rgb(48, 64, 96);
const COLOR_BORDER_ACTIVE: Color = Color::Rgb(99, 179, 237);
const COLOR_TEXT: Color = Color::Rgb(220, 226, 240);
const COLOR_TEXT_DIM: Color = Color::Rgb(100, 116, 140);
const COLOR_ACCENT: Color = Color::Rgb(99, 179, 237);
const COLOR_GREEN: Color = Color::Rgb(72, 199, 142);
const COLOR_YELLOW: Color = Color::Rgb(252, 196, 25);
const COLOR_RED: Color = Color::Rgb(252, 87, 87);
const COLOR_PURPLE: Color = Color::Rgb(180, 130, 255);
const COLOR_CYAN: Color = Color::Rgb(56, 210, 220);
const COLOR_CPU_FILL: Color = Color::Rgb(99, 179, 237);
const COLOR_MEM_FILL: Color = Color::Rgb(72, 199, 142);

// ─── Pane enum ───────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Execution,
    CallStack,
    Storage,
    Budget,
    Log,
    Diagnostics,
}

impl ActivePane {
    fn next(self) -> Self {
        match self {
            ActivePane::Execution => ActivePane::CallStack,
            ActivePane::CallStack => ActivePane::Storage,
            ActivePane::Storage => ActivePane::Budget,
            ActivePane::Budget => ActivePane::Log,
            ActivePane::Log => ActivePane::Diagnostics,
            ActivePane::Diagnostics => ActivePane::Execution,
        }
    }

    fn prev(self) -> Self {
        match self {
            ActivePane::Execution => ActivePane::Diagnostics,
            ActivePane::CallStack => ActivePane::Execution,
            ActivePane::Storage => ActivePane::CallStack,
            ActivePane::Budget => ActivePane::Storage,
            ActivePane::Log => ActivePane::Budget,
            ActivePane::Diagnostics => ActivePane::Log,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ActivePane::Execution => "Execution",
            ActivePane::CallStack => "Call Stack",
            ActivePane::Storage => "Storage",
            ActivePane::Budget => "Budget Meters",
            ActivePane::Log => "Execution Log",
            ActivePane::Diagnostics => "Diagnostics",
        }
    }
}

#[derive(Debug, Clone)]
struct PendingExecution {
    function: String,
    args: Option<String>,
}

// ─── TUI state ───────────────────────────────────────────────────────────────
pub struct DashboardApp {
    engine: DebuggerEngine,
    active_pane: ActivePane,

    // Execution pane
    pending_execution: Option<PendingExecution>,
    last_result: Option<String>,
    last_error: Option<String>,

    // Call stack pane
    call_stack_frames: Vec<CallFrame>,
    call_stack_state: ListState,

    // Storage pane
    storage_entries: Vec<(String, String)>,
    storage_state: ListState,
    storage_scroll_state: ScrollbarState,
    storage_filter: String,
    storage_selected: usize,
    storage_page_size: usize,
    storage_input_mode: Option<StorageInputMode>,
    storage_input_value: String,

    // Budget pane
    budget_info: BudgetInfo,
    budget_history_cpu: VecDeque<f64>,
    budget_history_mem: VecDeque<f64>,

    // Log pane
    log_entries: Vec<LogEntry>,
    log_scroll: usize,
    log_scroll_state: ScrollbarState,

    // Diagnostics pane
    diagnostics: Vec<crate::output::DiagnosticRecord>,
    diagnostics_state: ListState,
    diagnostics_scroll_state: ScrollbarState,

    // Misc
    last_refresh: Instant,
    step_count: usize,
    function_name: String,
    show_help: bool,
    status_message: Option<(String, StatusKind)>,
}

#[derive(Debug, Clone)]
struct LogEntry {
    timestamp: String,
    level: LogLevel,
    message: String,
}

#[derive(Debug, Clone, Copy)]
enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
    Step,
}

#[derive(Debug, Clone, Copy)]
enum StatusKind {
    Info,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageInputMode {
    Filter,
    Jump,
}

impl DashboardApp {
    pub fn new(engine: DebuggerEngine, function_name: String) -> Self {
        let pending_execution = if engine.is_paused() {
            engine.state().lock().ok().and_then(|state| {
                state.current_function().map(|f| PendingExecution {
                    function: f.to_string(),
                    args: state.current_args().map(str::to_string),
                })
            })
        } else {
            None
        };

        let mut app = Self {
            engine,
            active_pane: ActivePane::CallStack,

            pending_execution,
            last_result: None,
            last_error: None,

            call_stack_frames: Vec::new(),
            call_stack_state: {
                let mut state = ListState::default();
                state.select(Some(0));
                state
            },
            storage_entries: Vec::new(),
            storage_state: {
                let mut state = ListState::default();
                state.select(Some(0));
                state
            },
            storage_scroll_state: ScrollbarState::default().content_length(0),
            storage_filter: String::new(),
            storage_selected: 0,
            storage_page_size: 1,
            storage_input_mode: None,
            storage_input_value: String::new(),
            budget_info: BudgetInfo {
                cpu_instructions: 0,
                cpu_limit: 100_000_000,
                memory_bytes: 0,
                memory_limit: 40 * 1024 * 1024,
            },
            budget_history_cpu: VecDeque::with_capacity(60),
            budget_history_mem: VecDeque::with_capacity(60),
            log_entries: Vec::new(),
            log_scroll: 0,
            log_scroll_state: ScrollbarState::default().content_length(0),
            diagnostics: Vec::new(),
            diagnostics_state: {
                let mut state = ListState::default();
                state.select(Some(0));
                state
            },
            diagnostics_scroll_state: ScrollbarState::default().content_length(0),
            last_refresh: Instant::now(),
            step_count: 0,
            function_name,
            show_help: false,
            status_message: None,
        };

        app.push_log(
            LogLevel::Info,
            "TUI Dashboard initialized. Press ? for help.".to_string(),
        );
        app.push_log(
            LogLevel::Info,
            format!("Contract function: {}", app.function_name),
        );
        if let Some(pending) = &app.pending_execution {
            let args = pending.args.as_deref().unwrap_or("(none)");
            app.push_log(
                LogLevel::Info,
                format!("Staged: {} args={}", pending.function, args),
            );
            app.status_message = Some((
                "Press 'c' to execute staged call".to_string(),
                StatusKind::Info,
            ));
        }

        app.refresh_state();
        app
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn push_log(&mut self, level: LogLevel, message: String) {
        let timestamp = format_timestamp();
        self.log_entries.push(LogEntry {
            timestamp,
            level,
            message,
        });

        // Auto-scroll to bottom
        let len = self.log_entries.len();
        self.log_scroll = len.saturating_sub(1);
        self.log_scroll_state = self
            .log_scroll_state
            .content_length(len)
            .position(self.log_scroll);
    }

    fn refresh_state(&mut self) {
        // ── Call Stack ─────────────────────────────────────────────────
        if let Ok(state) = self.engine.state().lock() {
            let frames = state.call_stack().get_stack().to_vec();
            if frames.len() != self.call_stack_frames.len() {
                self.push_log(
                    LogLevel::Debug,
                    format!("Call stack depth: {}", frames.len()),
                );
            }
            self.call_stack_frames = frames;
            self.step_count = state.step_count();
        }

        // ── Budget ─────────────────────────────────────────────────────
        let new_budget =
            crate::inspector::budget::BudgetInspector::get_cpu_usage(self.engine.executor().host());

        let cpu_pct = new_budget.cpu_percentage();
        let mem_pct = new_budget.memory_percentage();

        if cpu_pct != self.budget_info.cpu_percentage() && cpu_pct > 80.0 {
            self.push_log(LogLevel::Warn, format!("CPU usage high: {:.1}%", cpu_pct));
        }
        self.budget_info = new_budget;

        if self.budget_history_cpu.len() >= 60 {
            self.budget_history_cpu.pop_front();
        }
        self.budget_history_cpu.push_back(cpu_pct);

        if self.budget_history_mem.len() >= 60 {
            self.budget_history_mem.pop_front();
        }
        self.budget_history_mem.push_back(mem_pct);

        // ── Storage ────────────────────────────────────────────────────
        let new_entries: Vec<(String, String)> = match self.engine.executor().get_storage_snapshot()
        {
            Ok(snapshot) => StorageInspector::sorted_entries_from_map(&snapshot),
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Storage snapshot failed: {}", e));
                Vec::new()
            }
        };

        if new_entries.len() != self.storage_entries.len() {
            self.push_log(
                LogLevel::Debug,
                format!("Storage entries: {}", new_entries.len()),
            );
        }
        self.storage_entries = new_entries;
        self.clamp_storage_selection();
        self.sync_storage_scroll_state();

        self.rebuild_diagnostics();
        self.last_refresh = Instant::now();
    }

    fn rebuild_diagnostics(&mut self) {
        let mut diagnostics = crate::output::collect_runtime_diagnostics(
            self.engine.source_map().is_some(),
            &self.budget_info,
            self.last_error.as_deref(),
        );

        let recent_alerts: Vec<_> = self
            .log_entries
            .iter()
            .filter(|entry| matches!(entry.level, LogLevel::Warn | LogLevel::Error))
            .collect();

        for entry in recent_alerts
            .into_iter()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let severity = match entry.level {
                LogLevel::Warn => crate::output::DiagnosticSeverity::Warning,
                LogLevel::Error => crate::output::DiagnosticSeverity::Error,
                _ => continue,
            };
            diagnostics.push(crate::output::DiagnosticRecord::new(
                "log",
                entry.message.clone(),
                Some(format!("Logged at {}", entry.timestamp)),
                severity,
            ));
        }

        let mut seen = HashSet::new();
        diagnostics.retain(|diagnostic| {
            seen.insert(format!(
                "{}|{}|{}",
                diagnostic.source,
                diagnostic.summary,
                diagnostic.severity.label()
            ))
        });

        self.diagnostics = diagnostics;
        let len = self.diagnostics.len();
        let selected = if len == 0 {
            None
        } else {
            Some(
                self.diagnostics_state
                    .selected()
                    .unwrap_or(0)
                    .min(len.saturating_sub(1)),
            )
        };
        self.diagnostics_state.select(selected);
        self.diagnostics_scroll_state = self
            .diagnostics_scroll_state
            .content_length(len)
            .position(selected.unwrap_or(0));
    }

    fn storage_query(&self, jump_to: Option<String>) -> StorageQuery {
        StorageQuery {
            filter: (!self.storage_filter.trim().is_empty()).then(|| self.storage_filter.clone()),
            jump_to,
            page: self.storage_selected / self.storage_page_size.max(1),
            page_size: self.storage_page_size.max(1),
        }
    }

    fn storage_page(&self) -> crate::inspector::storage::StoragePage {
        StorageInspector::build_page(&self.storage_entries, &self.storage_query(None))
    }

    fn storage_filtered_len(&self) -> usize {
        self.storage_page().filtered_entries
    }

    fn set_storage_page_size(&mut self, page_size: usize) {
        self.storage_page_size = page_size.max(1);
        self.clamp_storage_selection();
        self.sync_storage_scroll_state();
    }

    fn clamp_storage_selection(&mut self) {
        let filtered_len = self.storage_filtered_len();
        self.storage_selected = if filtered_len == 0 {
            0
        } else {
            self.storage_selected.min(filtered_len.saturating_sub(1))
        };
        self.storage_state
            .select((filtered_len > 0).then_some(self.storage_selected));
    }

    fn sync_storage_scroll_state(&mut self) {
        let filtered_len = self.storage_filtered_len();
        self.storage_scroll_state = self
            .storage_scroll_state
            .content_length(filtered_len)
            .position(self.storage_selected);
        self.storage_state
            .select((filtered_len > 0).then_some(self.storage_selected));
    }

    fn move_storage_selection(&mut self, delta: isize) {
        let filtered_len = self.storage_filtered_len();
        if filtered_len == 0 {
            self.storage_selected = 0;
        } else if delta.is_negative() {
            self.storage_selected = self.storage_selected.saturating_sub(delta.unsigned_abs());
        } else {
            self.storage_selected =
                (self.storage_selected + delta as usize).min(filtered_len.saturating_sub(1));
        }
        self.sync_storage_scroll_state();
    }

    fn move_storage_page(&mut self, delta: isize) {
        let step = self.storage_page_size.max(1) as isize;
        self.move_storage_selection(delta * step);
    }

    fn move_storage_to_boundary(&mut self, end: bool) {
        let filtered_len = self.storage_filtered_len();
        self.storage_selected = if end {
            filtered_len.saturating_sub(1)
        } else {
            0
        };
        self.sync_storage_scroll_state();
    }

    fn open_storage_input(&mut self, mode: StorageInputMode) {
        self.storage_input_mode = Some(mode);
        self.storage_input_value = match mode {
            StorageInputMode::Filter => self.storage_filter.clone(),
            StorageInputMode::Jump => String::new(),
        };
    }

    fn clear_storage_filter(&mut self) {
        self.storage_filter.clear();
        self.storage_input_mode = None;
        self.storage_input_value.clear();
        self.storage_selected = 0;
        self.sync_storage_scroll_state();
    }

    fn handle_storage_input_key(&mut self, key: KeyEvent) -> bool {
        let Some(mode) = self.storage_input_mode else {
            return false;
        };

        match key.code {
            KeyCode::Esc => {
                self.storage_input_mode = None;
                self.storage_input_value.clear();
            }
            KeyCode::Enter => {
                let value = self.storage_input_value.trim().to_string();
                match mode {
                    StorageInputMode::Filter => {
                        self.storage_filter = value;
                        self.storage_selected = 0;
                    }
                    StorageInputMode::Jump => {
                        let page = StorageInspector::build_page(
                            &self.storage_entries,
                            &StorageQuery {
                                filter: (!self.storage_filter.trim().is_empty())
                                    .then(|| self.storage_filter.clone()),
                                jump_to: Some(value),
                                page: 0,
                                page_size: self.storage_page_size.max(1),
                            },
                        );
                        if let Some(index) = page.jump_match_index {
                            self.storage_selected = index;
                        }
                    }
                }
                self.storage_input_mode = None;
                self.storage_input_value.clear();
                self.sync_storage_scroll_state();
            }
            KeyCode::Backspace => {
                self.storage_input_value.pop();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.storage_input_value.push(c);
            }
            _ => {}
        }

        true
    }

    // ── Step action ──────────────────────────────────────────────────────────
    fn do_step(&mut self) {
        match self.engine.step() {
            Ok(()) => {
                self.step_count += 1;
                self.push_log(
                    LogLevel::Step,
                    format!("Step #{} completed", self.step_count),
                );
            }
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Step failed: {}", e));
                self.status_message = Some((format!("Step error: {}", e), StatusKind::Error));
            }
        }
        self.refresh_state();
    }

    // ── Continue action ──────────────────────────────────────────────────────
    fn do_continue(&mut self) {
        if let Some(pending) = self.pending_execution.take() {
            self.push_log(LogLevel::Info, format!("Executing {}…", pending.function));
            match self
                .engine
                .execute_without_breakpoints(&pending.function, pending.args.as_deref())
            {
                Ok(output) => {
                    self.last_error = None;
                    self.last_result = Some(output.clone());
                    self.push_log(LogLevel::Info, format!("Result: {}", output));
                    self.status_message =
                        Some(("Execution complete".to_string(), StatusKind::Info));
                }
                Err(e) => {
                    self.last_result = None;
                    self.last_error = Some(e.to_string());
                    self.push_log(LogLevel::Error, format!("Execution failed: {}", e));
                    self.status_message =
                        Some((format!("Execution error: {}", e), StatusKind::Error));
                }
            }
        } else {
            match self.engine.continue_execution() {
                Ok(()) => {
                    self.push_log(LogLevel::Info, "Execution continuing…".to_string());
                    self.status_message = Some(("Running…".to_string(), StatusKind::Info));
                }
                Err(e) => {
                    self.push_log(LogLevel::Error, format!("Continue failed: {}", e));
                    self.status_message =
                        Some((format!("Continue error: {}", e), StatusKind::Error));
                }
            }
        }
        self.refresh_state();
    }

    // ── Scroll helpers ───────────────────────────────────────────────────────
    fn scroll_active_down(&mut self) {
        match self.active_pane {
            ActivePane::Execution => {}
            ActivePane::CallStack => {
                let len = self.call_stack_frames.len();
                if len == 0 {
                    return;
                }
                let sel = self.call_stack_state.selected().unwrap_or(0);
                self.call_stack_state.select(Some((sel + 1).min(len - 1)));
            }
            ActivePane::Storage => {
                self.move_storage_selection(1);
            }
            ActivePane::Log => {
                let len = self.log_entries.len();
                self.log_scroll = (self.log_scroll + 1).min(len.saturating_sub(1));
                self.log_scroll_state = self.log_scroll_state.position(self.log_scroll);
            }
            ActivePane::Budget => {}
            ActivePane::Diagnostics => {
                let len = self.diagnostics.len();
                if len == 0 {
                    return;
                }
                let sel = self.diagnostics_state.selected().unwrap_or(0);
                let new_sel = (sel + 1).min(len - 1);
                self.diagnostics_state.select(Some(new_sel));
                self.diagnostics_scroll_state = self.diagnostics_scroll_state.position(new_sel);
            }
        }
    }

    fn scroll_active_up(&mut self) {
        match self.active_pane {
            ActivePane::Execution => {}
            ActivePane::CallStack => {
                let sel = self.call_stack_state.selected().unwrap_or(0);
                self.call_stack_state.select(Some(sel.saturating_sub(1)));
            }
            ActivePane::Storage => {
                self.move_storage_selection(-1);
            }
            ActivePane::Log => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
                self.log_scroll_state = self.log_scroll_state.position(self.log_scroll);
            }
            ActivePane::Budget => {}
            ActivePane::Diagnostics => {
                let sel = self.diagnostics_state.selected().unwrap_or(0);
                let new_sel = sel.saturating_sub(1);
                self.diagnostics_state.select(Some(new_sel));
                self.diagnostics_scroll_state = self.diagnostics_scroll_state.position(new_sel);
            }
        }
    }
}

// ─── Main run loop ─────────────────────────────────────────────────────────

/// Launches the interactive TUI dashboard for contract debugging.
///
/// # Arguments
/// * `engine` - The debugger engine instance with contract state
/// * `function_name` - The name of the contract function being debugged
///
/// # Returns
/// Returns `Ok(())` on successful exit (via 'q' or Ctrl+C),
/// or a `DebuggerError` if terminal setup/teardown fails.
pub fn run_dashboard(engine: DebuggerEngine, function_name: &str) -> Result<()> {
    use crate::DebuggerError;

    if std::env::var_os("SOROBAN_DEBUG_TUI_SMOKE").is_some() {
        return run_dashboard_smoke(engine, function_name);
    }
    // Setup terminal
    enable_raw_mode()
        .map_err(|e| DebuggerError::IoError(format!("Failed to enable raw mode: {}", e)))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(|e| {
        DebuggerError::IoError(format!("Failed to execute terminal command: {}", e))
    })?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| DebuggerError::IoError(format!("Failed to create terminal: {}", e)))?;

    let res = run_app(&mut terminal, engine, function_name);

    // Restore terminal
    disable_raw_mode()
        .map_err(|e| DebuggerError::IoError(format!("Failed to disable raw mode: {}", e)))?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .map_err(|e| DebuggerError::IoError(format!("Failed to execute terminal command: {}", e)))?;
    terminal
        .show_cursor()
        .map_err(|e| DebuggerError::IoError(format!("Failed to show cursor: {}", e)))?;

    if let Err(err) = res {
        tracing::error!("TUI error: {:?}", err);
    }

    Ok(())
}

fn run_dashboard_smoke(engine: DebuggerEngine, function_name: &str) -> Result<()> {
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| DebuggerError::IoError(format!("Failed to create terminal: {}", e)))?;

    let mut app = DashboardApp::new(engine, function_name.to_string());
    app.do_continue();

    terminal
        .draw(|f| ui(f, &mut app))
        .map_err(|e| DebuggerError::IoError(format!("Failed to draw terminal: {}", e)))?;

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    engine: DebuggerEngine,
    function_name: &str,
) -> Result<()> {
    let mut app = DashboardApp::new(engine, function_name.to_string());
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        terminal
            .draw(|f| ui(f, &mut app))
            .map_err(|e| DebuggerError::IoError(format!("Failed to draw terminal: {}", e)))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_default();

        if event::poll(timeout)
            .map_err(|e| DebuggerError::IoError(format!("Failed to poll event: {}", e)))?
        {
            if let Event::Key(key) = event::read()
                .map_err(|e| DebuggerError::IoError(format!("Failed to read event: {}", e)))?
            {
                // Ctrl-C always exits
                if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                    return Ok(());
                }

                if app.handle_storage_input_key(key) {
                    continue;
                }

                match key.code {
                    // ── Quit ─────────────────────────────────────
                    KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(()),

                    // ── Help overlay toggle ───────────────────────
                    KeyCode::Char('?') => {
                        app.show_help = !app.show_help;
                    }

                    // ── Pane navigation ───────────────────────────
                    KeyCode::Tab => {
                        app.active_pane = app.active_pane.next();
                    }
                    KeyCode::BackTab => {
                        app.active_pane = app.active_pane.prev();
                    }
                    KeyCode::Char('1') => app.active_pane = ActivePane::Execution,
                    KeyCode::Char('2') => app.active_pane = ActivePane::CallStack,
                    KeyCode::Char('3') => app.active_pane = ActivePane::Storage,
                    KeyCode::Char('4') => app.active_pane = ActivePane::Budget,
                    KeyCode::Char('5') => app.active_pane = ActivePane::Log,
                    KeyCode::Char('6') => app.active_pane = ActivePane::Diagnostics,

                    // ── Scroll ────────────────────────────────────
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.scroll_active_down();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.scroll_active_up();
                    }

                    // ── Debugger actions ──────────────────────────
                    KeyCode::PageDown if app.active_pane == ActivePane::Storage => {
                        app.move_storage_page(1);
                    }
                    KeyCode::PageUp if app.active_pane == ActivePane::Storage => {
                        app.move_storage_page(-1);
                    }
                    KeyCode::Home if app.active_pane == ActivePane::Storage => {
                        app.move_storage_to_boundary(false);
                    }
                    KeyCode::End if app.active_pane == ActivePane::Storage => {
                        app.move_storage_to_boundary(true);
                    }
                    KeyCode::Char('/') if app.active_pane == ActivePane::Storage => {
                        app.open_storage_input(StorageInputMode::Filter);
                    }
                    KeyCode::Char('g') if app.active_pane == ActivePane::Storage => {
                        app.open_storage_input(StorageInputMode::Jump);
                    }
                    KeyCode::Char('x') | KeyCode::Esc if app.active_pane == ActivePane::Storage => {
                        app.clear_storage_filter();
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        app.do_step();
                    }
                    KeyCode::Char('c') => {
                        app.do_continue();
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        app.refresh_state();
                        app.push_log(LogLevel::Info, "Manually refreshed state.".to_string());
                    }

                    _ => {}
                }
            }
        }

        // Periodic refresh
        if last_tick.elapsed() >= tick_rate {
            app.refresh_state();
            last_tick = Instant::now();
        }
    }
}

// ─── Drawing ──────────────────────────────────────────────────────────────
fn ui(f: &mut Frame, app: &mut DashboardApp) {
    let area = f.size();

    // Background
    f.render_widget(Block::default().style(Style::default().bg(COLOR_BG)), area);

    // ── Outer layout: header + body + footer ──────────────────────────────
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(0),    // body
            Constraint::Length(1), // status bar
        ])
        .split(area);

    render_header(f, app, outer[0]);
    render_body(f, app, outer[1]);
    render_status_bar(f, app, outer[2]);

    // Help overlay
    if app.show_help {
        render_help_overlay(f, area);
    }
}

// ─── Header ───────────────────────────────────────────────────────────────
fn render_header(f: &mut Frame, app: &DashboardApp, area: Rect) {
    let title_line = Line::from(vec![
        Span::styled(
            " ◆ SOROBAN DEBUGGER ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(COLOR_BORDER)),
        Span::styled(
            format!(" fn: {} ", app.function_name),
            Style::default()
                .fg(COLOR_PURPLE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(COLOR_BORDER)),
        Span::styled(
            format!(
                " CPU: {:.1}%  MEM: {:.1}% ",
                app.budget_info.cpu_percentage(),
                app.budget_info.memory_percentage()
            ),
            Style::default()
                .fg(gauge_color(app.budget_info.cpu_percentage()))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(COLOR_BORDER)),
        Span::styled(
            format!(" Steps: {} ", app.step_count),
            Style::default().fg(COLOR_CYAN),
        ),
        Span::styled("│ ", Style::default().fg(COLOR_BORDER)),
        Span::styled(
            " [?]Help  [q]Quit  [Tab]Pane  [s]Step  [c]Continue ",
            Style::default().fg(COLOR_TEXT_DIM),
        ),
    ]);

    let header = Paragraph::new(title_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(COLOR_ACCENT))
                .style(Style::default().bg(COLOR_SURFACE)),
        )
        .alignment(Alignment::Left);

    f.render_widget(header, area);
}

// ─── Body (4 panes) ─────────────────────────────────────────────────────
fn render_body(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    if area.width >= 150 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(28),
                Constraint::Percentage(47),
                Constraint::Percentage(25),
            ])
            .split(area);

        let left_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[0]);

        let center_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Percentage(45),
                Constraint::Percentage(55),
            ])
            .split(columns[1]);

        render_call_stack(f, app, left_column[0]);
        render_budget(f, app, left_column[1]);
        render_execution(f, app, center_column[0]);
        render_storage(f, app, center_column[1]);
        render_log(f, app, center_column[2]);
        render_diagnostics(f, app, columns[2]);
    } else {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(area);

        let left_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Percentage(27),
                Constraint::Percentage(35),
            ])
            .split(columns[0]);

        let right_column = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Percentage(45),
                Constraint::Percentage(55),
            ])
            .split(columns[1]);

        render_call_stack(f, app, left_column[0]);
        render_budget(f, app, left_column[1]);
        render_diagnostics(f, app, left_column[2]);
        render_execution(f, app, right_column[0]);
        render_storage(f, app, right_column[1]);
        render_log(f, app, right_column[2]);
    }
}

// ─── Call Stack pane ──────────────────────────────────────────────────────
fn render_execution(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Execution;
    let block = pane_block("  Execution", "1", is_active);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (current_fn, current_args) = app
        .engine
        .state()
        .lock()
        .ok()
        .map(|s| {
            (
                s.current_function()
                    .unwrap_or(&app.function_name)
                    .to_string(),
                s.current_args().map(str::to_string),
            )
        })
        .unwrap_or_else(|| (app.function_name.clone(), None));

    let status = if app.pending_execution.is_some() {
        "Staged (press 'c' to run)"
    } else if app.last_result.is_some() {
        "Completed"
    } else if app.last_error.is_some() {
        "Error"
    } else {
        "Idle"
    };

    let paused = app.engine.is_paused();
    let arg_text = current_args.as_deref().unwrap_or("(none)");

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(COLOR_TEXT_DIM)),
            Span::styled(status, Style::default().fg(COLOR_ACCENT)),
            Span::styled("  │  ", Style::default().fg(COLOR_BORDER)),
            Span::styled("Paused: ", Style::default().fg(COLOR_TEXT_DIM)),
            Span::styled(
                format!("{}", paused),
                Style::default().fg(if paused { COLOR_YELLOW } else { COLOR_GREEN }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Fn: ", Style::default().fg(COLOR_TEXT_DIM)),
            Span::styled(current_fn, Style::default().fg(COLOR_PURPLE)),
        ]),
        Line::from(vec![
            Span::styled("Args: ", Style::default().fg(COLOR_TEXT_DIM)),
            Span::styled(arg_text, Style::default().fg(COLOR_TEXT)),
        ]),
    ];

    // Show paused file/line if available
    if paused {
        if let Some(reason) = app.engine.pause_reason_label() {
            lines.push(Line::from(vec![
                Span::styled("Pause reason: ", Style::default().fg(COLOR_TEXT_DIM)),
                Span::styled(reason, Style::default().fg(COLOR_YELLOW)),
            ]));
        }
        if let Some(loc) = app.engine.current_source_location() {
            let file = loc.file.display();
            let line = loc.line;
            let col = loc.column.map(|c| format!(":{}", c)).unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled("Paused at: ", Style::default().fg(COLOR_TEXT_DIM)),
                Span::styled(
                    format!("{}:{}{}", file, line, col),
                    Style::default().fg(COLOR_YELLOW),
                ),
            ]));
        }
    }

    if let Some(result) = &app.last_result {
        lines.push(Line::from(vec![
            Span::styled("Result: ", Style::default().fg(COLOR_TEXT_DIM)),
            Span::styled(result.clone(), Style::default().fg(COLOR_GREEN)),
        ]));
    } else if let Some(err) = &app.last_error {
        lines.push(Line::from(vec![
            Span::styled("Error: ", Style::default().fg(COLOR_TEXT_DIM)),
            Span::styled(err.clone(), Style::default().fg(COLOR_RED)),
        ]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "Result: (none yet)",
            Style::default().fg(COLOR_TEXT_DIM),
        )]));
    }

    let exec_widget = Paragraph::new(lines)
        .style(Style::default().bg(COLOR_SURFACE))
        .wrap(Wrap { trim: true });
    f.render_widget(exec_widget, inner);
}

fn render_call_stack(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::CallStack;
    let block = pane_block("  Call Stack", "2", is_active);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.call_stack_frames.is_empty() {
        let empty = Paragraph::new(Line::from(vec![Span::styled(
            "  (empty — no execution active)",
            Style::default().fg(COLOR_TEXT_DIM),
        )]))
        .style(Style::default().bg(COLOR_SURFACE));
        f.render_widget(empty, inner);
        return;
    }

    let depth = app.call_stack_frames.len();
    let items: Vec<ListItem> = app
        .call_stack_frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            let is_top = i == depth - 1;
            let indent = "  ".repeat(i);
            let arrow = if is_top { "→ " } else { "└─ " };

            let contract_ctx = frame
                .contract_id
                .as_ref()
                .map(|c| format!(" [{}]", shorten_id(c)))
                .unwrap_or_default();

            let dur_ctx = frame
                .duration
                .map(|d| format!(" ({:.2}ms)", d.as_secs_f64() * 1000.0))
                .unwrap_or_default();

            let func_color = if is_top { COLOR_ACCENT } else { COLOR_TEXT };
            let frame_style = if is_top {
                Style::default()
                    .fg(func_color)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::Rgb(25, 35, 55))
            } else {
                Style::default().fg(func_color)
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{}{}", indent, arrow),
                    Style::default().fg(COLOR_TEXT_DIM),
                ),
                Span::styled(frame.function.clone(), frame_style),
                Span::styled(contract_ctx, Style::default().fg(COLOR_PURPLE)),
                Span::styled(dur_ctx, Style::default().fg(COLOR_TEXT_DIM)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 50, 80))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, inner, &mut app.call_stack_state);
}

// ─── Storage pane ─────────────────────────────────────────────────────────
fn render_storage(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Storage;
    let count = app.storage_entries.len();
    let matched = app.storage_filtered_len();
    let title = if app.storage_filter.trim().is_empty() {
        format!("  Storage  ({} entries)", count)
    } else {
        format!("  Storage  ({} / {} entries)", matched, count)
    };
    let block = pane_block(&title, "3", is_active);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let header_rows = if inner.height > 2 { 2 } else { 1 };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_rows), Constraint::Min(0)])
        .split(inner);
    let list_region = sections[1];

    app.set_storage_page_size(list_region.height.max(1) as usize);
    let page = app.storage_page();
    let summary = if page.filtered_entries == 0 {
        "  No matching storage entries".to_string()
    } else {
        format!(
            "  Page {}/{}  showing {}-{} of {}",
            page.page + 1,
            page.total_pages,
            page.page_start + 1,
            page.page_start + page.entries.len(),
            page.filtered_entries
        )
    };
    let filter_line = if app.storage_filter.trim().is_empty() {
        "  /=filter  g=jump  PgUp/PgDn=page  Home/End=edges  x=clear".to_string()
    } else {
        format!(
            "  filter={}  /=edit  g=jump  PgUp/PgDn=page  x=clear",
            truncate(
                &app.storage_filter,
                sections[0].width.saturating_sub(10) as usize
            )
        )
    };
    let meta = Paragraph::new(vec![
        Line::from(Span::styled(summary, Style::default().fg(COLOR_TEXT_DIM))),
        Line::from(Span::styled(
            filter_line,
            Style::default().fg(COLOR_TEXT_DIM),
        )),
    ])
    .style(Style::default().bg(COLOR_SURFACE));
    f.render_widget(meta, sections[0]);

    if app.storage_entries.is_empty() {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            "  (no storage captured — run a contract to populate)",
            Style::default().fg(COLOR_TEXT_DIM),
        )]))
        .style(Style::default().bg(COLOR_SURFACE))
        .wrap(Wrap { trim: false });
        f.render_widget(msg, list_region);
        return;
    }

    if page.entries.is_empty() {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            "  (no storage entries match the current filter)",
            Style::default().fg(COLOR_TEXT_DIM),
        )]))
        .style(Style::default().bg(COLOR_SURFACE))
        .wrap(Wrap { trim: false });
        f.render_widget(msg, list_region);
        if let Some(mode) = app.storage_input_mode {
            render_storage_prompt(f, area, mode, &app.storage_input_value);
        }
        return;
    }

    let items: Vec<ListItem> = page
        .entries
        .iter()
        .enumerate()
        .map(|(offset, (k, v))| {
            // Truncate long keys/values to fit
            let max_key = (list_region.width as usize).saturating_sub(14).min(32);
            let max_val = (list_region.width as usize).saturating_sub(max_key + 12);
            let key_display = truncate(k, max_key);
            let val_display = truncate(v, max_val);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>4} ", page.page_start + offset + 1),
                    Style::default().fg(COLOR_TEXT_DIM),
                ),
                Span::styled(
                    key_display,
                    Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" = ", Style::default().fg(COLOR_TEXT_DIM)),
                Span::styled(val_display, Style::default().fg(COLOR_TEXT)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 55, 55))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    // Scrollbar area
    let scroll_area = Rect {
        x: list_region.x + list_region.width.saturating_sub(1),
        y: list_region.y,
        width: 1,
        height: list_region.height,
    };
    let list_area = Rect {
        width: list_region.width.saturating_sub(1),
        ..list_region
    };

    f.render_stateful_widget(list, list_area, &mut app.storage_state);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .style(Style::default().fg(COLOR_BORDER)),
        scroll_area,
        &mut app.storage_scroll_state,
    );

    if let Some(mode) = app.storage_input_mode {
        render_storage_prompt(f, area, mode, &app.storage_input_value);
    }
}

// ─── Budget pane ──────────────────────────────────────────────────────────
fn render_storage_prompt(f: &mut Frame, area: Rect, mode: StorageInputMode, input: &str) {
    let popup_width = 64u16.min(area.width.saturating_sub(4));
    let popup_height = 5u16.min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(popup_width) / 2;
    let y = area.y + area.height.saturating_sub(popup_height) / 2;
    let popup = Rect::new(x, y, popup_width, popup_height);
    let title = match mode {
        StorageInputMode::Filter => " Storage Filter ",
        StorageInputMode::Jump => " Jump To Key ",
    };
    let hint = match mode {
        StorageInputMode::Filter => "Type a substring, prefix*, or re:pattern. Enter applies.",
        StorageInputMode::Jump => "Type a key or prefix. Enter jumps to the first match.",
    };

    let widget = Paragraph::new(vec![
        Line::from(Span::styled(hint, Style::default().fg(COLOR_TEXT_DIM))),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(input.to_string(), Style::default().fg(COLOR_TEXT)),
        ]),
    ])
    .block(
        Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(COLOR_ACCENT)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(COLOR_ACCENT))
            .style(Style::default().bg(COLOR_SURFACE)),
    );

    f.render_widget(widget, popup);
}

fn render_budget(f: &mut Frame, app: &DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Budget;
    let block = pane_block("  Budget Meters", "4", is_active);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2), // CPU label
            Constraint::Length(1), // CPU gauge
            Constraint::Length(1), // spacer
            Constraint::Length(2), // MEM label
            Constraint::Length(1), // MEM gauge
            Constraint::Length(1), // spacer
            Constraint::Min(0),    // details
        ])
        .split(inner);

    // ── CPU ─────────────────────────────────────────────────────────
    let cpu_pct = app.budget_info.cpu_percentage();
    let cpu_color = gauge_color(cpu_pct);
    let cpu_label = Paragraph::new(Line::from(vec![
        Span::styled(
            "  CPU Instructions  ",
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{:>12} / {:<12}",
                fmt_num(app.budget_info.cpu_instructions),
                fmt_num(app.budget_info.cpu_limit)
            ),
            Style::default().fg(COLOR_TEXT_DIM),
        ),
        Span::styled(
            format!("  {:>6.2}%", cpu_pct),
            Style::default().fg(cpu_color).add_modifier(Modifier::BOLD),
        ),
    ]));
    f.render_widget(cpu_label, rows[0]);

    let cpu_gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(COLOR_CPU_FILL)
                .bg(Color::Rgb(30, 40, 60)),
        )
        .percent(cpu_pct.min(100.0) as u16)
        .label(Span::styled(
            format!("{:.1}%", cpu_pct),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(cpu_gauge, rows[1]);

    // ── MEM ─────────────────────────────────────────────────────────
    let mem_pct = app.budget_info.memory_percentage();
    let mem_color = gauge_color(mem_pct);
    let mem_label = Paragraph::new(Line::from(vec![
        Span::styled(
            "  Memory Bytes      ",
            Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{:>12} / {:<12}",
                fmt_bytes(app.budget_info.memory_bytes),
                fmt_bytes(app.budget_info.memory_limit)
            ),
            Style::default().fg(COLOR_TEXT_DIM),
        ),
        Span::styled(
            format!("  {:>6.2}%", mem_pct),
            Style::default().fg(mem_color).add_modifier(Modifier::BOLD),
        ),
    ]));
    f.render_widget(mem_label, rows[3]);

    let mem_gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(COLOR_MEM_FILL)
                .bg(Color::Rgb(20, 45, 35)),
        )
        .percent(mem_pct.min(100.0) as u16)
        .label(Span::styled(
            format!("{:.1}%", mem_pct),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(mem_gauge, rows[4]);

    // ── Trend sparkline (ASCII) ──────────────────────────────────────
    if rows[6].height >= 1 {
        let sparkline_row = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1); 2])
            .split(rows[6]);

        let cpu_spark = build_sparkline(&app.budget_history_cpu, "CPU trend: ", COLOR_CPU_FILL);
        let mem_spark = build_sparkline(&app.budget_history_mem, "MEM trend: ", COLOR_MEM_FILL);

        if !sparkline_row.is_empty() {
            f.render_widget(Paragraph::new(cpu_spark), sparkline_row[0]);
        }
        if sparkline_row.len() > 1 {
            f.render_widget(Paragraph::new(mem_spark), sparkline_row[1]);
        }
    }
}

fn build_sparkline(history: &VecDeque<f64>, prefix: &str, color: Color) -> Line<'static> {
    let bar_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let spark: String = history
        .iter()
        .map(|&pct| {
            let idx = ((pct / 100.0) * (bar_chars.len() as f64 - 1.0)) as usize;
            bar_chars[idx.min(bar_chars.len() - 1)]
        })
        .collect();

    Line::from(vec![
        Span::styled(format!("  {}", prefix), Style::default().fg(COLOR_TEXT_DIM)),
        Span::styled(spark, Style::default().fg(color)),
    ])
}

// ─── Log pane ─────────────────────────────────────────────────────────────
fn render_log(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Log;
    let count = app.log_entries.len();
    let title = format!("  Execution Log  ({} events)", count);
    let block = pane_block(&title, "5", is_active);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.log_entries.is_empty() {
        let msg =
            Paragraph::new("  (no log entries yet)").style(Style::default().fg(COLOR_TEXT_DIM));
        f.render_widget(msg, inner);
        return;
    }

    // Determine the window of lines to show
    let visible_height = inner.height as usize;
    let total = app.log_entries.len();

    // Keep scroll in bounds
    if app.log_scroll >= total {
        app.log_scroll = total.saturating_sub(1);
    }

    let start = if total > visible_height {
        app.log_scroll.min(total - visible_height)
    } else {
        0
    };
    let end = (start + visible_height).min(total);

    let lines: Vec<Line> = app.log_entries[start..end]
        .iter()
        .map(|entry| {
            let (level_str, level_color) = match entry.level {
                LogLevel::Info => (" INFO ", COLOR_ACCENT),
                LogLevel::Warn => (" WARN ", COLOR_YELLOW),
                LogLevel::Error => (" ERR  ", COLOR_RED),
                LogLevel::Debug => (" DBG  ", COLOR_TEXT_DIM),
                LogLevel::Step => (" STEP ", COLOR_GREEN),
            };
            Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.timestamp),
                    Style::default().fg(COLOR_TEXT_DIM),
                ),
                Span::styled(
                    level_str,
                    Style::default()
                        .fg(Color::Black)
                        .bg(level_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(entry.message.clone(), Style::default().fg(COLOR_TEXT)),
            ])
        })
        .collect();

    // Scrollbar
    let scroll_area = Rect {
        x: inner.x + inner.width.saturating_sub(1),
        y: inner.y,
        width: 1,
        height: inner.height,
    };
    let text_area = Rect {
        width: inner.width.saturating_sub(1),
        ..inner
    };

    let log_widget = Paragraph::new(lines).style(Style::default().bg(COLOR_SURFACE));
    f.render_widget(log_widget, text_area);

    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .style(Style::default().fg(COLOR_BORDER)),
        scroll_area,
        &mut app.log_scroll_state,
    );
}

// ─── Status bar ───────────────────────────────────────────────────────────
fn render_diagnostics(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Diagnostics;
    let title = format!("  Diagnostics  ({} active)", app.diagnostics.len());
    let block = pane_block(&title, "6", is_active);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.diagnostics.is_empty() {
        let msg = Paragraph::new(vec![
            Line::from(Span::styled(
                "  No active diagnostics.",
                Style::default().fg(COLOR_GREEN),
            )),
            Line::from(Span::styled(
                "  Warnings and notices will appear here.",
                Style::default().fg(COLOR_TEXT_DIM),
            )),
        ])
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        return;
    }

    let items: Vec<ListItem> = app
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let severity_color = match diagnostic.severity {
                crate::output::DiagnosticSeverity::Notice => COLOR_ACCENT,
                crate::output::DiagnosticSeverity::Warning => COLOR_YELLOW,
                crate::output::DiagnosticSeverity::Error => COLOR_RED,
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", diagnostic.severity.label()),
                        Style::default()
                            .fg(Color::Black)
                            .bg(severity_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        diagnostic.source.to_uppercase(),
                        Style::default()
                            .fg(COLOR_TEXT_DIM)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(
                    format!(" {}", diagnostic.summary),
                    Style::default().fg(COLOR_TEXT),
                )),
            ];

            if let Some(detail) = &diagnostic.detail {
                lines.push(Line::from(Span::styled(
                    format!(" {}", detail),
                    Style::default().fg(COLOR_TEXT_DIM),
                )));
            }

            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(45, 50, 72))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("â–¶ ");

    let scroll_area = Rect {
        x: inner.x + inner.width.saturating_sub(1),
        y: inner.y,
        width: 1,
        height: inner.height,
    };
    let list_area = Rect {
        width: inner.width.saturating_sub(1),
        ..inner
    };

    f.render_stateful_widget(list, list_area, &mut app.diagnostics_state);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("â†‘"))
            .end_symbol(Some("â†“"))
            .style(Style::default().fg(COLOR_BORDER)),
        scroll_area,
        &mut app.diagnostics_scroll_state,
    );
}

fn render_status_bar(f: &mut Frame, app: &DashboardApp, area: Rect) {
    let active_label = app.active_pane.label();
    let (msg, msg_color) = if let Some((ref s, kind)) = app.status_message {
        let c = match kind {
            StatusKind::Info => COLOR_ACCENT,
            StatusKind::Error => COLOR_RED,
        };
        (s.as_str(), c)
    } else {
        ("Ready", COLOR_GREEN)
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" ◆ Active: {} ", active_label),
            Style::default()
                .fg(COLOR_ACCENT)
                .bg(COLOR_SURFACE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(COLOR_BORDER).bg(COLOR_SURFACE)),
        Span::styled(
            format!(" {} ", msg),
            Style::default().fg(msg_color).bg(COLOR_SURFACE),
        ),
        Span::styled(
            " │ Tab=next pane  ↑↓/jk=scroll  s=step  c=continue  r=refresh  q=quit ",
            Style::default().fg(COLOR_TEXT_DIM).bg(COLOR_SURFACE),
        ),
    ]);

    let bar = Paragraph::new(line).style(Style::default().bg(COLOR_SURFACE));
    f.render_widget(bar, area);
}

// ─── Help overlay ─────────────────────────────────────────────────────────
fn render_help_overlay(f: &mut Frame, area: Rect) {
    // Center a 60×22 box
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = 24u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(popup_width)) / 2 + area.x;
    let y = (area.height.saturating_sub(popup_height)) / 2 + area.y;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(
        Block::default().style(Style::default().bg(COLOR_BG)),
        popup_area,
    );

    let help_lines = vec![
        Line::from(Span::styled(
            "  Keyboard Reference",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Navigation",
            Style::default()
                .fg(COLOR_PURPLE)
                .add_modifier(Modifier::BOLD),
        )]),
        bind("Tab / Shift+Tab", "Cycle panes forward / backward"),
        bind("1 / 2 / 3 / 4 / 5", "Jump directly to pane"),
        bind("↑ / k", "Scroll active pane up"),
        bind("↓ / j", "Scroll active pane down"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Debugger Actions",
            Style::default()
                .fg(COLOR_PURPLE)
                .add_modifier(Modifier::BOLD),
        )]),
        bind("s / S", "Step (one instruction)"),
        bind("c", "Continue execution"),
        bind("r / R", "Refresh state manually"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  General",
            Style::default()
                .fg(COLOR_PURPLE)
                .add_modifier(Modifier::BOLD),
        )]),
        bind("?", "Toggle this help overlay"),
        bind("q / Q", "Quit dashboard"),
        bind("Ctrl+C", "Force quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Press ? again to close",
            Style::default().fg(COLOR_TEXT_DIM),
        )]),
    ];

    let help_widget = Paragraph::new(help_lines)
        .block(
            Block::default()
                .title(Span::styled(
                    " Help ",
                    Style::default()
                        .fg(COLOR_ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(COLOR_ACCENT))
                .style(Style::default().bg(COLOR_SURFACE)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(help_widget, popup_area);
}

fn bind(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("    "),
        Span::styled(
            format!("{:<20}", key),
            Style::default()
                .fg(COLOR_YELLOW)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(COLOR_TEXT)),
    ])
}

// ─── Shared block builder ─────────────────────────────────────────────────
fn pane_block(title: &str, num: &str, is_active: bool) -> Block<'static> {
    let border_color = if is_active {
        COLOR_BORDER_ACTIVE
    } else {
        COLOR_BORDER
    };
    let title_str = format!("{}  [{}]", title, num);
    Block::default()
        .title(Span::styled(
            title_str,
            Style::default()
                .fg(if is_active {
                    COLOR_ACCENT
                } else {
                    COLOR_TEXT_DIM
                })
                .add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ))
        .borders(Borders::ALL)
        .border_type(if is_active {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(COLOR_SURFACE))
}

// ─── Utilities ────────────────────────────────────────────────────────────
fn format_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, s)
}

fn gauge_color(pct: f64) -> Color {
    if pct >= 90.0 {
        COLOR_RED
    } else if pct >= 70.0 {
        COLOR_YELLOW
    } else {
        COLOR_GREEN
    }
}

fn fmt_num(n: u64) -> String {
    // Insert thousands separators
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push('_');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn fmt_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        "...".to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn shorten_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…{}", &id[..6], &id[id.len() - 4..])
    } else {
        id.to_string()
    }
}
