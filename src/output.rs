//! Output and accessibility configuration for screen-reader compatible CLI.
//!
//! Supports `NO_COLOR` (disable ANSI colors) and `--no-unicode` (ASCII-only output).

use std::sync::atomic::{AtomicBool, Ordering};

static NO_UNICODE: AtomicBool = AtomicBool::new(false);
static COLORS_ENABLED: AtomicBool = AtomicBool::new(true);

/// Global output/accessibility configuration.
pub struct OutputConfig;

impl OutputConfig {
    /// Configure from CLI flags and environment.
    /// Call once at startup after parsing args.
    pub fn configure(no_unicode: bool) {
        NO_UNICODE.store(no_unicode, Ordering::Relaxed);
        // NO_COLOR: if set and not empty, disable ANSI colors
        let no_color = std::env::var("NO_COLOR")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        COLORS_ENABLED.store(!no_color, Ordering::Relaxed);
    }

    /// Whether `--no-unicode` is active (use ASCII-only output).
    #[inline]
    pub fn no_unicode() -> bool {
        NO_UNICODE.load(Ordering::Relaxed)
    }

    /// Whether ANSI colors are enabled (false when NO_COLOR is set and not empty).
    #[inline]
    pub fn colors_enabled() -> bool {
        COLORS_ENABLED.load(Ordering::Relaxed)
    }

    /// Replace box-drawing and other Unicode symbols with ASCII when `--no-unicode` is set.
    pub fn to_ascii(s: &str) -> String {
        if !Self::no_unicode() {
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            out.push(Self::replace_unicode_char(c));
        }
        out
    }

    fn replace_unicode_char(c: char) -> char {
        match c {
            // Corners
            '┌' | '┐' | '└' | '┘' => '+',
            // Horizontal
            '─' | '━' | '═' => '-',
            // Vertical
            '│' | '┃' => '|',
            // T-junctions
            '┬' | '┴' | '├' | '┤' | '┼' => '+',
            // Bullets / markers
            '•' => '*',
            '→' => '>',
            '⚠' => '!',
            '✔' | '✓' => '+',
            '✗' | '✘' => 'x',
            _ => c,
        }
    }

    /// Horizontal rule character(s) for section separators.
    pub fn rule_char() -> &'static str {
        if Self::no_unicode() {
            "-"
        } else {
            "-"
        }
    }

    /// Double-line rule character for headers.
    pub fn double_rule_char() -> &'static str {
        if Self::no_unicode() {
            "="
        } else {
            "\u{2550}" // ═
        }
    }

    /// A horizontal rule line (single line, for section separators).
    pub fn rule_line(len: usize) -> String {
        Self::rule_char().repeat(len)
    }

    /// A double horizontal rule line (for headers).
    pub fn double_rule_line(len: usize) -> String {
        Self::double_rule_char().repeat(len)
    }
}

/// Status kind for text-equivalent labels (screen reader friendly).
#[derive(Clone, Copy)]
pub enum StatusLabel {
    Pass,
    Fail,
    Info,
    Warning,
    Error,
    Working,
}

impl StatusLabel {
    /// Text label to use when color is disabled or for accessibility.
    pub fn as_str(self) -> &'static str {
        match self {
            StatusLabel::Pass => "[PASS]",
            StatusLabel::Fail => "[FAIL]",
            StatusLabel::Info => "[INFO]",
            StatusLabel::Warning => "[WARN]",
            StatusLabel::Error => "[ERROR]",
            StatusLabel::Working => "[WORKING...]",
        }
    }
}

/// Spinner / progress: in no-unicode or accessibility mode, return static text instead of Unicode spinner.
pub fn spinner_text() -> &'static str {
    if OutputConfig::no_unicode() {
        "[WORKING...]"
    } else {
        "[WORKING...]"
    }
}
