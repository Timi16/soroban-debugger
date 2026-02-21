//! `inspect` sub-command – print WASM module stats and embedded contract
//! metadata to stdout.

use std::{fs, path::Path};

use crate::{
    utils::wasm::{
        extract_contract_metadata, get_module_info,
        parse_function_signatures,
    },
    InspectArgs, Result,
};
use colored::Colorize;
use serde::Serialize;

const BAR_WIDTH: usize = 54;

// ─── public entry point ───────────────────────────────────────────────────────

/// CLI entry point for the `inspect` sub-command.
pub fn run(args: &InspectArgs) -> Result<()> {
    let wasm_bytes = fs::read(&args.contract).map_err(|e| {
        anyhow::anyhow!(
            "Cannot read WASM file '{}': {e}",
            args.contract.display()
        )
    })?;

    if args.json {
        print_json_report(&args.contract, &wasm_bytes)
    } else {
        println!();
        print_report(&args.contract, &wasm_bytes)
    }
}

#[derive(Serialize)]
struct FullReport {
    file: String,
    size_bytes: usize,
    module_info: crate::utils::wasm::ModuleInfo,
    functions: Vec<String>,
    metadata: crate::utils::wasm::ContractMetadata,
}

fn print_json_report(path: &Path, wasm_bytes: &[u8]) -> Result<()> {
    let info = get_module_info(wasm_bytes)?;
    let functions = parse_functions(wasm_bytes)?;
    let metadata = extract_contract_metadata(wasm_bytes)?;

    let report = FullReport {
        file: path.display().to_string(),
        size_bytes: wasm_bytes.len(),
        module_info: info,
        functions,
        metadata,
    };

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

// ─── report ───────────────────────────────────────────────────────────────────

/// Render a full inspection report for `wasm_bytes` to stdout.
///
/// This function is deliberately kept separate from `run` so that tests can
/// drive it without touching the filesystem.
fn print_report(path: &Path, wasm_bytes: &[u8]) -> Result<()> {
    let info       = get_module_info(wasm_bytes)?;
    let signatures = parse_function_signatures(wasm_bytes)?;
    let metadata   = extract_contract_metadata(wasm_bytes)?;

    let heavy = "═".repeat(BAR_WIDTH);

    // ── header ────────────────────────────────────────────────────────────────
    println!("{heavy}");
    println!("  {}", "Soroban Contract Inspector".bold().cyan());
    println!("{heavy}");
    println!();
    println!("  File : {}", path.display().to_string().bright_white());
    println!("  Size : {} ({:.2} KB)", 
        format!("{} bytes", wasm_bytes.len()).bright_white(),
        wasm_bytes.len() as f64 / 1024.0
    );
    println!();

    // ── module stats ──────────────────────────────────────────────────────────
    section_header("Module Statistics");
    println!("  Types      : {}", info.type_count.to_string().bright_white());
    println!("  Functions  : {}", info.function_count.to_string().bright_white());
    println!("  Exports    : {}", info.export_count.to_string().bright_white());
    println!();

    // ── section breakdown ─────────────────────────────────────────────────────
    section_header("WASM Section Breakdown");
    println!("  {:<20} | {:>10} | {:>6}", "Section", "Size", "Total%");
    println!("  {}|{}|{}", "─".repeat(21), "─".repeat(12), "─".repeat(8));

    for section in &info.sections {
        let percentage = (section.size as f64 / info.total_size as f64) * 100.0;
        let size_str = format!("{} B", section.size);
        
        let row = format!("  {:<20} | {:>10} | {:>5.1}%", 
            section.name, 
            size_str,
            percentage
        );

        // Highlight sections over 50KB or more than 50% of total
        if section.size > 50 * 1024 || percentage > 50.0 {
            println!("{}", row.red().bold());
        } else if section.size > 10 * 1024 {
            println!("{}", row.yellow());
        } else {
            println!("{}", row.bright_white());
        }
    }
    println!();

    // ── function signatures ───────────────────────────────────────────────────
    section_header("Exported Functions");
    if signatures.is_empty() {
        println!("  (no contractspecv0 section found)");
    } else {
        let name_w = signatures.iter().map(|s| s.name.len()).max().unwrap_or(8);
        println!("  {:<name_w$}  Signature", "Function", name_w = name_w);
        println!("  {}  {}", "─".repeat(name_w), "─".repeat(BAR_WIDTH - name_w - 4));

        for sig in &signatures {
            let params: Vec<String> = sig
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.type_name))
                .collect();

            let ret = match &sig.return_type {
                Some(t) if t != "Void" => format!(" -> {t}"),
                _                      => String::new(),
            };

            println!(
                "  {:<name_w$}  ({}){ret}",
                sig.name,
                params.join(", "),
                name_w = name_w,
            );
        }
    }
    println!();

    // ── contract metadata ─────────────────────────────────────────────────────
    section_header("Contract Metadata");
    if metadata.is_empty() {
        println!("  ⚠  No metadata section embedded in this contract.");
    } else {
        print_field("Contract Version", &metadata.contract_version);
        print_field("SDK Version",      &metadata.sdk_version);
        print_field("Build Date",       &metadata.build_date);
        print_field("Author / Org",     &metadata.author);
        print_field("Description",      &metadata.description);
        print_field("Implementation",   &metadata.implementation);
    }

    println!("{heavy}");
    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn section_header(title: &str) {
    // "─── Title ──────" where total width equals BAR_WIDTH.
    let fill = BAR_WIDTH.saturating_sub(title.len() + 5);
    println!("─── {title} {}", "─".repeat(fill));
}

/// Print a labelled row only when the value is `Some`.
fn print_field(label: &str, value: &Option<String>) {
    if let Some(v) = value {
        // Left-align the label in a 20-char column for consistent spacing.
        println!("  {label:<20} : {v}");
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── minimal WASM helpers (mirrors the helpers in utils::wasm tests) ────────

    fn uleb128(mut v: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut b = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
        out
    }

    fn wasm_with_custom_section(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]);
        bytes.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        bytes.push(0x00); // custom section id

        let mut section = Vec::new();
        section.extend_from_slice(&uleb128(name.len()));
        section.extend_from_slice(name.as_bytes());
        section.extend_from_slice(payload);

        bytes.extend_from_slice(&uleb128(section.len()));
        bytes.extend_from_slice(&section);
        bytes
    }

    fn bare_wasm() -> Vec<u8> {
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    // ── report tests ──────────────────────────────────────────────────────────

    /// The report must never error for a contract that has no metadata section.
    #[test]
    fn report_on_metadata_absent_wasm_succeeds() {
        let result = print_report(Path::new("test.wasm"), &bare_wasm());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// The report must never error when metadata IS present.
    #[test]
    fn report_on_metadata_present_wasm_succeeds() {
        let json =
            r#"{"contract_version":"2.0.0","sdk_version":"22.0.0","author":"Acme Corp"}"#;
        let wasm = wasm_with_custom_section("contractmeta", json.as_bytes());
        let result = print_report(Path::new("test.wasm"), &wasm);
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    /// Partial metadata (only some fields present) must render without errors.
    #[test]
    fn report_on_partial_metadata_succeeds() {
        let json = r#"{"contract_version":"0.1.0"}"#;
        let wasm = wasm_with_custom_section("contractmeta", json.as_bytes());
        let result = print_report(Path::new("partial.wasm"), &wasm);
        assert!(result.is_ok());
    }

    // ── print_field helper ────────────────────────────────────────────────────

    #[test]
    fn print_field_with_none_does_not_panic() {
        print_field("Any Label", &None);
    }

    #[test]
    fn print_field_with_some_does_not_panic() {
        print_field("Any Label", &Some("a value".to_string()));
    }
}