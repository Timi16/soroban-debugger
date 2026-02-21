use crate::debugger::engine::DebuggerEngine;
use crate::inspector::budget::BudgetInfo;
use crate::inspector::stack::CallFrame;
use crate::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
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
    collections::VecDeque,
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
    CallStack,
    Storage,
    Budget,
    Timeline,
    Source,
    Log,
}

impl ActivePane {
    fn next(self) -> Self {
        match self {
            ActivePane::CallStack => ActivePane::Storage,
            ActivePane::Storage => ActivePane::Budget,
            ActivePane::Budget => ActivePane::Timeline,
            ActivePane::Timeline => ActivePane::Source,
            ActivePane::Source => ActivePane::Log,
            ActivePane::Log => ActivePane::CallStack,
        }
    }

    fn prev(self) -> Self {
        match self {
            ActivePane::CallStack => ActivePane::Log,
            ActivePane::Storage => ActivePane::CallStack,
            ActivePane::Budget => ActivePane::Storage,
            ActivePane::Timeline => ActivePane::Budget,
            ActivePane::Source => ActivePane::Timeline,
            ActivePane::Log => ActivePane::Source,
        }
    }

    fn label(self) -> &'static str {
        match self {
            ActivePane::CallStack => "Call Stack",
            ActivePane::Storage => "Storage",
            ActivePane::Budget => "Budget Meters",
            ActivePane::Timeline => "Timeline",
            ActivePane::Source => "Source Code",
            ActivePane::Log => "Execution Log",
        }
    }
}

// ─── TUI state ───────────────────────────────────────────────────────────────
pub struct DashboardApp {
    engine: DebuggerEngine,
    active_pane: ActivePane,

    // Call stack pane
    call_stack_frames: Vec<CallFrame>,
    call_stack_state: ListState,

    // Storage pane
    storage_entries: Vec<(String, String)>,
    storage_state: ListState,
    storage_scroll_state: ScrollbarState,

    // Budget pane
    budget_info: BudgetInfo,
    budget_history_cpu: VecDeque<f64>,
    budget_history_mem: VecDeque<f64>,

    // Log pane
    log_entries: Vec<LogEntry>,
    log_scroll: usize,
    log_scroll_state: ScrollbarState,

    // Timeline pane
    timeline_state: ListState,

    // Source pane
    source_scroll: usize,
    source_scroll_state: ScrollbarState,

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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

impl DashboardApp {
    pub fn new(engine: DebuggerEngine, function_name: String) -> Self {
        let mut storage_scroll_state = ScrollbarState::default();
        storage_scroll_state = storage_scroll_state.content_length(0);
        let mut log_scroll_state = ScrollbarState::default();
        log_scroll_state = log_scroll_state.content_length(0);

        let mut storage_state = ListState::default();
        storage_state.select(Some(0));

        let mut call_stack_state = ListState::default();
        call_stack_state.select(Some(0));

        let mut app = Self {
            engine,
            active_pane: ActivePane::CallStack,
            call_stack_frames: Vec::new(),
            call_stack_state,
            storage_entries: Vec::new(),
            storage_state,
            storage_scroll_state,
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
            log_scroll_state,
            timeline_state: ListState::default(),
            source_scroll: 0,
            source_scroll_state: ScrollbarState::default(),
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
            format!("Contract function: {}", app.function_name.clone()),
        );

        app.refresh_state();
        app
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn push_log(&mut self, level: LogLevel, message: String) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs();
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        let timestamp = format!("{:02}:{:02}:{:02}", hours, mins, s);
        self.log_entries.push(LogEntry {
            timestamp,
            level,
            message,
        });
        // auto-scroll to bottom
        let len = self.log_entries.len();
        if len > 0 {
            self.log_scroll = len.saturating_sub(1);
        }
        let content_len = self.log_entries.len();
        self.log_scroll_state = self.log_scroll_state.content_length(content_len);
        self.log_scroll_state = self.log_scroll_state.position(self.log_scroll);
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

        if cpu_pct != self.budget_info.cpu_percentage() {
            if cpu_pct > 80.0 {
                self.push_log(LogLevel::Warn, format!("CPU usage high: {:.1}%", cpu_pct));
            }
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
        // Storage displayed from the engine's internal inspector
        let new_entries: Vec<(String, String)> = {
            let inspector = crate::inspector::StorageInspector::new();
            let all = inspector.get_all();
            let mut v: Vec<(String, String)> =
                all.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };

        if new_entries.len() != self.storage_entries.len() {
            self.push_log(
                LogLevel::Debug,
                format!("Storage entries: {}", new_entries.len()),
            );
        }
        self.storage_entries = new_entries;
        let slen = self.storage_entries.len();
        self.storage_scroll_state = self.storage_scroll_state.content_length(slen);

        self.last_refresh = Instant::now();
    }

    // ── Step action ──────────────────────────────────────────────────────────
    fn do_step(&mut self) {
        match self.engine.step() {
            Ok(()) => {
                self.push_log(
                    LogLevel::Step,
                    format!("Step #{} completed", self.engine.state().lock().unwrap().step_count()),
                );
            }
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Step failed: {}", e));
                self.status_message = Some((format!("Step error: {}", e), StatusKind::Error));
            }
        }
        self.refresh_state();
    }

    // ── Step Source action ───────────────────────────────────────────────────
    fn do_step_source(&mut self) {
        match self.engine.step_source() {
            Ok(stepped) => {
                if stepped {
                    self.push_log(LogLevel::Step, "Step Source completed".to_string());
                    // Auto-scroll to current line
                    if let Some(loc) = self.engine.current_source_location() {
                        self.source_scroll = loc.line.saturating_sub(10) as usize;
                    }
                } else {
                    self.push_log(LogLevel::Warn, "At end of execution".to_string());
                }
            }
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Step Source failed: {}", e));
            }
        }
        self.refresh_state();
    }

    // ── Step Back action ─────────────────────────────────────────────────────
    fn do_step_back(&mut self) {
        match self.engine.step_back() {
            Ok(stepped) => {
                if stepped {
                    self.push_log(LogLevel::Step, "Step back completed".to_string());
                } else {
                    self.push_log(LogLevel::Warn, "At earliest point in history".to_string());
                }
            }
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Step back failed: {}", e));
            }
        }
        self.refresh_state();
    }

    // ── Continue Back action ─────────────────────────────────────────────────
    fn do_continue_back(&mut self) {
        match self.engine.continue_back() {
            Ok(()) => {
                self.push_log(LogLevel::Step, "Continued back to breakpoint or beginning".to_string());
            }
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Continue back failed: {}", e));
            }
        }
        self.refresh_state();
    }

    // ── Continue action ──────────────────────────────────────────────────────
    fn do_continue(&mut self) {
        match self.engine.continue_execution() {
            Ok(()) => {
                self.push_log(LogLevel::Info, "Execution continuing…".to_string());
                self.status_message = Some(("Running…".to_string(), StatusKind::Info));
            }
            Err(e) => {
                self.push_log(LogLevel::Error, format!("Continue failed: {}", e));
                self.status_message = Some((format!("Continue error: {}", e), StatusKind::Error));
            }
        }
        self.refresh_state();
    }

    // ── Scroll helpers ───────────────────────────────────────────────────────
    fn scroll_active_down(&mut self) {
        match self.active_pane {
            ActivePane::CallStack => {
                let len = self.call_stack_frames.len();
                if len == 0 {
                    return;
                }
                let sel = self.call_stack_state.selected().unwrap_or(0);
                self.call_stack_state.select(Some((sel + 1).min(len - 1)));
            }
            ActivePane::Storage => {
                let len = self.storage_entries.len();
                if len == 0 {
                    return;
                }
                let sel = self.storage_state.selected().unwrap_or(0);
                let new_sel = (sel + 1).min(len - 1);
                self.storage_state.select(Some(new_sel));
                self.storage_scroll_state = self.storage_scroll_state.position(new_sel);
            }
            ActivePane::Log => {
                let len = self.log_entries.len();
                self.log_scroll = (self.log_scroll + 1).min(len.saturating_sub(1));
                self.log_scroll_state = self.log_scroll_state.position(self.log_scroll);
            }
            ActivePane::Budget => {}
            ActivePane::Timeline => {
                let len = self.engine.get_timeline().len();
                if len == 0 { return; }
                let sel = self.timeline_state.selected().unwrap_or(0);
                self.timeline_state.select(Some((sel + 1).min(len - 1)));
            }
            ActivePane::Source => {
                self.source_scroll = self.source_scroll.saturating_add(1);
            }
        }
    }

    fn scroll_active_up(&mut self) {
        match self.active_pane {
            ActivePane::CallStack => {
                let sel = self.call_stack_state.selected().unwrap_or(0);
                self.call_stack_state.select(Some(sel.saturating_sub(1)));
            }
            ActivePane::Storage => {
                let sel = self.storage_state.selected().unwrap_or(0);
                let new_sel = sel.saturating_sub(1);
                self.storage_state.select(Some(new_sel));
                self.storage_scroll_state = self.storage_scroll_state.position(new_sel);
            }
            ActivePane::Log => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
                self.log_scroll_state = self.log_scroll_state.position(self.log_scroll);
            }
            ActivePane::Budget => {}
            ActivePane::Timeline => {
                let len = self.engine.get_timeline().len();
                if len == 0 { return; }
                let sel = self.timeline_state.selected().unwrap_or(0);
                self.timeline_state.select(Some(sel.saturating_sub(1)));
            }
            ActivePane::Source => {
                self.source_scroll = self.source_scroll.saturating_sub(1);
            }
        }
    }
}

// ─── Main run loop ─────────────────────────────────────────────────────────
pub fn run_dashboard(engine: DebuggerEngine, function_name: &str) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, engine, function_name);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("TUI error: {:?}", err);
    }

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
        terminal.draw(|f| ui(f, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_default();

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                // Ctrl-C always exits
                if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                    return Ok(());
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
                    KeyCode::Char('1') => app.active_pane = ActivePane::CallStack,
                    KeyCode::Char('2') => app.active_pane = ActivePane::Storage,
                    KeyCode::Char('3') => app.active_pane = ActivePane::Budget,
                    KeyCode::Char('4') => app.active_pane = ActivePane::Timeline,
                    KeyCode::Char('5') => app.active_pane = ActivePane::Source,
                    KeyCode::Char('6') => app.active_pane = ActivePane::Log,

                    // ── Scroll ────────────────────────────────────
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.scroll_active_down();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.scroll_active_up();
                    }

                    // ── Debugger actions ──────────────────────────
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        app.do_step();
                    }
                    KeyCode::Char('v') | KeyCode::Char('V') => {
                        app.do_step_source();
                    }
                    KeyCode::Char('c') => {
                        app.do_continue();
                    }
                    KeyCode::Char('b') | KeyCode::Char('B') | KeyCode::Backspace => {
                        app.do_step_back();
                    }
                    KeyCode::Char('l') | KeyCode::Char('L') => {
                        app.do_continue_back();
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
    // Split body into left column (top=call stack, bottom=budget) and right column (top=storage, bottom=log)
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let left_column = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(columns[0]);

    let right_column = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(columns[1]);

    render_call_stack(f, app, left_column[0]);
    render_budget(f, app, left_column[1]);
    render_timeline(f, app, left_column[2]);
    render_storage(f, app, right_column[0]);
    render_source(f, app, right_column[1]);
    render_log(f, app, right_column[2]);
}

// ─── Call Stack pane ──────────────────────────────────────────────────────
fn render_call_stack(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::CallStack;
    let block = pane_block("  Call Stack", "1", is_active);

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
    let title = format!("  Storage  ({} entries)", count);
    let block = pane_block(&title, "2", is_active);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.storage_entries.is_empty() {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            "  (no storage captured — run a contract to populate)",
            Style::default().fg(COLOR_TEXT_DIM),
        )]))
        .style(Style::default().bg(COLOR_SURFACE))
        .wrap(Wrap { trim: false });
        f.render_widget(msg, inner);
        return;
    }

    let items: Vec<ListItem> = app
        .storage_entries
        .iter()
        .map(|(k, v)| {
            // Truncate long keys/values to fit
            let max_key = (inner.width as usize).saturating_sub(6).min(25);
            let max_val = (inner.width as usize).saturating_sub(max_key + 6);
            let key_display = truncate(k, max_key);
            let val_display = truncate(v, max_val);
            ListItem::new(Line::from(vec![
                Span::styled(" ", Style::default()),
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
        x: inner.x + inner.width.saturating_sub(1),
        y: inner.y,
        width: 1,
        height: inner.height,
    };
    let list_area = Rect {
        width: inner.width.saturating_sub(1),
        ..inner
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
}

// ─── Budget pane ──────────────────────────────────────────────────────────
fn render_budget(f: &mut Frame, app: &DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Budget;
    let block = pane_block("  Budget Meters", "3", is_active);

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

        if sparkline_row.len() > 0 {
            f.render_widget(Paragraph::new(cpu_spark), sparkline_row[0]);
        }
        if sparkline_row.len() > 1 {
            f.render_widget(Paragraph::new(mem_spark), sparkline_row[1]);
        }
    }
}

// ─── Timeline pane ─────────────────────────────────────────────────────────
fn render_timeline(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Timeline;
    let timeline = app.engine.get_timeline();
    let count = timeline.len();
    let current_pos = timeline.current_pos();

    let block = pane_block(&format!("  Timeline ({} snapshots)", count), "4", is_active);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if count == 0 {
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            "  (no history recorded yet — step to capture state)",
            Style::default().fg(COLOR_TEXT_DIM),
        )]))
        .style(Style::default().bg(COLOR_SURFACE));
        f.render_widget(msg, inner);
        return;
    }

    let items: Vec<ListItem> = timeline
        .get_history()
        .iter()
        .enumerate()
        .map(|(i, snap)| {
            let is_current = i == current_pos;
            let style = if is_current {
                Style::default()
                    .fg(COLOR_GREEN)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::Rgb(25, 45, 35))
            } else {
                Style::default().fg(COLOR_TEXT)
            };

            let prefix = if is_current { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}Snapshot {:>3}", prefix, i), style),
                Span::styled(
                    format!(" [Step {}]", snap.step),
                    Style::default().fg(COLOR_CYAN),
                ),
                Span::styled(
                    format!(" fn: {}", shorten_func(&snap.function)),
                    Style::default().fg(COLOR_PURPLE),
                ),
                Span::styled(
                    format!(" [IP:{}]", snap.instruction_index),
                    Style::default().fg(COLOR_TEXT_DIM),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::Rgb(30, 40, 50))
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(list, inner, &mut app.timeline_state);
}

// ─── Source pane ───────────────────────────────────────────────────────────
fn render_source(f: &mut Frame, app: &mut DashboardApp, area: Rect) {
    let is_active = app.active_pane == ActivePane::Source;
    let loc = app.engine.current_source_location();

    let title = if let Some(l) = &loc {
        format!("  Source: {} ", shorten_path(&l.file))
    } else {
        "  Source (no debug info) ".to_string()
    };

    let block = pane_block(&title, "5", is_active);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(l) = loc {
        let content = app.engine.source_map_mut().get_source_content(&l.file);
        if let Some(src) = content {
            let lines: Vec<Line> = src
                .lines()
                .enumerate()
                .skip(app.source_scroll)
                .take(inner.height as usize)
                .map(|(i, line)| {
                    let line_num = i + 1;
                    let is_current = line_num == l.line as usize;
                    
                    let mut spans = vec![
                        Span::styled(
                            format!("{:>4} │ ", line_num),
                            Style::default().fg(COLOR_TEXT_DIM),
                        ),
                        Span::raw(line),
                    ];

                    if is_current {
                        Line::from(spans).style(
                            Style::default()
                                .bg(Color::Rgb(40, 40, 60))
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Line::from(spans)
                    }
                })
                .collect();

            let paragraph = Paragraph::new(lines)
                .style(Style::default().bg(COLOR_SURFACE));
            f.render_widget(paragraph, inner);
        } else {
            let msg = Paragraph::new(format!("  Could not load source file: {:?}", l.file))
                .style(Style::default().fg(COLOR_RED));
            f.render_widget(msg, inner);
        }
    } else {
        let msg = Paragraph::new("  (debug info missing or not enabled)")
            .style(Style::default().fg(COLOR_TEXT_DIM));
        f.render_widget(msg, inner);
    }
}

fn shorten_path(path: &std::path::Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn shorten_func(name: &str) -> String {
    if name.len() > 15 {
        format!("{}…", &name[..14])
    } else {
        name.to_string()
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
    let block = pane_block(&title, "4", is_active);

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
fn render_status_bar(f: &mut Frame, app: &DashboardApp, area: Rect) {
    let active_label = app.active_pane.label();
    let (msg, msg_color) = if let Some((ref s, kind)) = app.status_message {
        let c = match kind {
            StatusKind::Info => COLOR_ACCENT,
            StatusKind::Success => COLOR_GREEN,
            StatusKind::Warning => COLOR_YELLOW,
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
            " │ s=step  v=src-step  b=back  c=cont  l=cont-back  Tab=pane  q=quit ",
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
        bind("1 / 2 / 3 / 4", "Jump directly to pane"),
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
        bind("v / V", "Step SOURCE line"),
        bind("b / Backspace", "Step BACK in time"),
        bind("c", "Continue execution"),
        bind("l / L", "Continue BACKWARDS"),
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
