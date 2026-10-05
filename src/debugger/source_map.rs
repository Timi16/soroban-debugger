use crate::{DebuggerError, Result};
use gimli::{Dwarf, EndianSlice, RunTimeEndian};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use wasmparser::{Parser, Payload};

const DWARF_SECTION_NAMES: &[&str] = &[
    ".debug_info",
    ".debug_abbrev",
    ".debug_line",
    ".debug_str",
    ".debug_line_str",
    ".debug_ranges",
    ".debug_rnglists",
    ".debug_addr",
    ".debug_str_offsets",
];

/// FNV-1a 64-bit hash of `data`.  Used as a fast fingerprint to detect whether
/// the WASM bytes have changed since the last parse.  No external dependency needed.
fn fnv1a_hash(data: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET_BASIS;
    for &byte in data {
        h ^= byte as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Represents a source code location
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
}

/// A diagnostic message indicating an issue with loading DWARF debug metadata.
///
/// # Diagnostic construction style
///
/// - Static messages (no runtime values): use `"text".to_string()`.
/// - Dynamic messages (interpolated values): use `format!("text {}", value)`.
/// - Never use `format!("static string")` — this triggers `clippy::useless_format`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceMapDiagnostic {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceMapSectionStatus {
    pub name: String,
    pub present: bool,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceMapMappingPreview {
    pub offset: usize,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceMapInspectionReport {
    pub mappings_count: usize,
    pub preview: Vec<SourceMapMappingPreview>,
    pub sections: Vec<SourceMapSectionStatus>,
    pub diagnostics: Vec<SourceMapDiagnostic>,
    pub fallback_mode: String,
    pub fallback_message: String,
    pub units_processed: usize,
    pub units_with_line_program: usize,
    pub coverage_ratio: f64,
}

/// Manages mapping from WASM offsets to source code locations
pub struct SourceMap {
    /// Mapping from offset to source location (sorted by offset)
    offsets: BTreeMap<usize, SourceLocation>,
    /// Cache of source file contents
    source_cache: HashMap<PathBuf, String>,
    /// Code section payload range (when known), used to normalize DWARF addresses.
    code_section_range: Option<std::ops::Range<usize>>,
    /// FNV-1a hash of the WASM bytes from the last successful parse.
    /// A matching hash on a subsequent `load()` call means the bytes have not
    /// changed and the existing `offsets` can be reused without re-parsing DWARF.
    last_wasm_hash: Option<u64>,
    /// Number of full DWARF parses performed.  Starts at zero and is only
    /// incremented when a cache miss triggers a real parse.  Exposed for
    /// testing so callers can verify that repeated loads with the same bytes
    /// do not re-parse.
    parse_count: usize,
    /// Diagnostics accumulated during DWARF parsing
    pub diagnostics: Vec<SourceMapDiagnostic>,
    pub units_processed: usize,
    pub units_with_line_program: usize,
}

/// Result of resolving a source breakpoint (file + line) to a concrete source location.
///
/// Supports exact line breakpoints.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceBreakpointResolution {
    /// The requested 1-based source line.
    pub requested_line: u32,
    /// The resolved 1-based source line (may be adjusted to the next executable line).
    pub line: u32,
    /// Whether the breakpoint binding is considered exact/high-confidence.
    pub verified: bool,
    /// Formatted file:line location for the debugger engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    /// Stable reason code when `verified` is false.
    pub reason_code: String,
    /// Human readable explanation for UI.
    pub message: String,
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMap {
    /// Create a new empty source map
    pub fn new() -> Self {
        Self {
            offsets: BTreeMap::new(),
            source_cache: HashMap::new(),
            code_section_range: None,
            last_wasm_hash: None,
            parse_count: 0,
            diagnostics: Vec::new(),
            units_processed: 0,
            units_with_line_program: 0,
        }
    }

    /// Load debug info from WASM bytes and build the mapping.
    ///
    /// If the incoming bytes have the same FNV-1a hash as the bytes from the
    /// last successful parse, the existing `offsets` are returned immediately
    /// without re-parsing any DWARF sections.  `parse_count()` is only
    /// incremented on a real (cache-miss) parse.
    pub fn load(&mut self, wasm_bytes: &[u8]) -> Result<()> {
        let hash = fnv1a_hash(wasm_bytes);

        // Cache hit: bytes unchanged — skip the expensive DWARF walk.
        if self.last_wasm_hash == Some(hash) {
            return Ok(());
        }

        // Cache miss: clear stale data and re-parse.
        self.offsets.clear();
        self.last_wasm_hash = None;
        self.units_processed = 0;
        self.units_with_line_program = 0;
        self.code_section_range = crate::utils::wasm::code_section_range(wasm_bytes)?;

        let mut custom_sections: HashMap<String, &[u8]> = HashMap::new();
        for payload in Parser::new(0).parse_all(wasm_bytes) {
            let payload = payload.map_err(|e| {
                DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e))
            })?;
            if let Payload::CustomSection(reader) = payload {
                custom_sections.insert(reader.name().to_string(), reader.data());
            }
        }

        let load_section = |id: gimli::SectionId| -> std::result::Result<EndianSlice<RunTimeEndian>, gimli::Error> {
            let name = id.name();
            let data = custom_sections
                .get(name)
                .or_else(|| custom_sections.get(&format!(".{}", name)))
                .or_else(|| custom_sections.get(name.trim_start_matches('.')))
                .copied()
                .unwrap_or(&[]);

            Ok(EndianSlice::new(data, RunTimeEndian::Little))
        };

        let dwarf = match Dwarf::load(&load_section) {
            Ok(d) => d,
            Err(e) => {
                self.diagnostics.push(SourceMapDiagnostic {
                    message: format!("Failed to load DWARF sections: {}", e),
                });
                // We cannot proceed without the main DWARF sections headers successfully parsed
                return Err(DebuggerError::WasmLoadError(format!(
                    "DWARF sections severely malformed: {}",
                    e
                ))
                .into());
            }
        };

        let mut units = dwarf.units();
        loop {
            let header = match units.next() {
                Ok(Some(h)) => h,
                Ok(None) => break,
                Err(e) => {
                    self.diagnostics.push(SourceMapDiagnostic {
                        message: format!("Failed to read DWARF unit header: {}", e),
                    });
                    break;
                }
            };

            let unit = match dwarf.unit(header) {
                Ok(u) => u,
                Err(e) => {
                    self.diagnostics.push(SourceMapDiagnostic {
                        message: format!("Failed to load DWARF unit content: {}", e),
                    });
                    continue; // try next unit
                }
            };

            self.units_processed += 1;
            if unit.line_program.is_some() {
                self.units_with_line_program += 1;
            }

            if let Some(program) = unit.line_program.clone() {
                let mut rows = program.rows();
                loop {
                    let (header, row) = match rows.next_row() {
                        Ok(Some(t)) => t,
                        Ok(None) => break,
                        Err(e) => {
                            self.diagnostics.push(SourceMapDiagnostic {
                                message: format!("Failed to read DWARF line row: {}", e),
                            });
                            break; // break row iteration for this unit, continue to next unit
                        }
                    };

                    if let Some(file_path) =
                        self.get_file_path(&dwarf, &unit, header, row.file_index())
                    {
                        let (offset, alt_offset) =
                            self.normalize_wasm_offsets(row.address() as usize, wasm_bytes.len());
                        let line = row.line().map(|l| l.get() as u32).unwrap_or(0);
                        let column = match row.column() {
                            gimli::ColumnType::LeftEdge => None,
                            gimli::ColumnType::Column(column) => Some(column.get() as u32),
                        };
                        let location = SourceLocation {
                            file: file_path,
                            line,
                            column,
                        };

                        self.offsets.insert(offset, location.clone());
                        if let Some(alt_offset) = alt_offset {
                            self.offsets.entry(alt_offset).or_insert(location);
                        }
                    }
                }
            } else {
                self.diagnostics.push(SourceMapDiagnostic {
                    message: "DWARF unit is missing a line program (e.g., .debug_line section data missing or malformed).".to_string(),
                });
            }
        }

        self.last_wasm_hash = Some(hash);
        self.parse_count += 1;
        Ok(())
    }

    pub fn inspect_wasm(
        wasm_bytes: &[u8],
        preview_limit: usize,
    ) -> Result<SourceMapInspectionReport> {
        let section_sizes = dwarf_section_sizes(wasm_bytes)?;
        let sections = DWARF_SECTION_NAMES
            .iter()
            .map(|name| SourceMapSectionStatus {
                name: (*name).to_string(),
                present: section_sizes.contains_key(*name),
                size_bytes: section_sizes.get(*name).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>();

        let mut source_map = SourceMap::new();
        let load_result = source_map.load(wasm_bytes);
        let diagnostics = source_map.diagnostics.clone();
        let preview = source_map
            .mappings()
            .take(preview_limit)
            .map(|(offset, location)| SourceMapMappingPreview {
                offset,
                location: location.clone(),
            })
            .collect::<Vec<_>>();
        let mappings_count = source_map.len();

        let missing_sections = sections
            .iter()
            .filter(|section| !section.present)
            .map(|section| section.name.as_str())
            .collect::<Vec<_>>();

        let (fallback_mode, fallback_message) = if mappings_count > 0 {
            if diagnostics.is_empty() {
                (
                    "source".to_string(),
                    "DWARF line mappings resolved successfully.".to_string(),
                )
            } else {
                (
                    "partial-source".to_string(),
                    "DWARF mappings were resolved, but some debug metadata was missing or malformed; the debugger may fall back to less precise location data for unmapped code.".to_string(),
                )
            }
        } else if missing_sections.is_empty() {
            (
                "wasm-only".to_string(),
                "No executable source mappings were produced; the debugger will fall back to WASM-only behavior.".to_string(),
            )
        } else {
            (
                "wasm-only".to_string(),
                format!(
                    "Missing DWARF sections ({}) prevent source-level mapping; the debugger will fall back to WASM-only behavior.",
                    missing_sections.join(", ")
                ),
            )
        };

        match load_result {
            Ok(()) => Ok(SourceMapInspectionReport {
                mappings_count,
                preview,
                sections,
                diagnostics,
                fallback_mode,
                fallback_message,
                units_processed: source_map.units_processed,
                units_with_line_program: source_map.units_with_line_program,
                coverage_ratio: if source_map.units_processed > 0 {
                    source_map.units_with_line_program as f64 / source_map.units_processed as f64
                } else {
                    0.0
                },
            }),
            Err(err) => {
                if mappings_count == 0 && diagnostics.is_empty() {
                    return Err(err);
                }

                // load() failed partway through: any mappings collected are
                // partial. Downgrade "source" → "partial-source" so the mode
                // accurately reflects that the parse did not complete.
                let (fallback_mode, fallback_message) =
                    if mappings_count > 0 && fallback_mode == "source" {
                        (
                            "partial-source".to_string(),
                            "DWARF parse failed after collecting some mappings; \
                         results may be incomplete."
                                .to_string(),
                        )
                    } else {
                        (fallback_mode, fallback_message)
                    };

                Ok(SourceMapInspectionReport {
                    mappings_count,
                    preview,
                    sections,
                    diagnostics,
                    fallback_mode,
                    fallback_message,
                    units_processed: source_map.units_processed,
                    units_with_line_program: source_map.units_with_line_program,
                    coverage_ratio: if source_map.units_processed > 0 {
                        source_map.units_with_line_program as f64
                            / source_map.units_processed as f64
                    } else {
                        0.0
                    },
                })
            }
        }
    }

    /// Returns `true` if no mappings were loaded.
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Number of mapped offsets.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Iterate over mappings as `(offset, location)` pairs.
    pub fn mappings(&self) -> impl Iterator<Item = (usize, &SourceLocation)> {
        self.offsets.iter().map(|(o, l)| (*o, l))
    }

    fn normalize_wasm_offsets(
        &self,
        dwarf_address: usize,
        wasm_len: usize,
    ) -> (usize, Option<usize>) {
        let Some(code_range) = &self.code_section_range else {
            return (dwarf_address, None);
        };

        let code_start = code_range.start;
        let code_end = code_range.end;
        let code_len = code_range.end.saturating_sub(code_range.start);
        let module_candidate = dwarf_address;

        let relative_candidate = dwarf_address
            .checked_add(code_start)
            .filter(|candidate| *candidate < wasm_len);
        let module_in_code = module_candidate >= code_start
            && module_candidate < code_end
            && module_candidate < wasm_len;
        let relative_in_code = dwarf_address < code_len
            && relative_candidate
                .map(|candidate| candidate >= code_start && candidate < code_end)
                .unwrap_or(false);

        match (module_in_code, relative_in_code) {
            (true, true) => (module_candidate, relative_candidate),
            (true, false) => (module_candidate, None),
            (false, true) => (relative_candidate.unwrap_or(module_candidate), None),
            (false, false) => {
                if module_candidate < wasm_len {
                    (module_candidate, None)
                } else if let Some(relative_candidate) = relative_candidate {
                    (relative_candidate, None)
                } else {
                    (dwarf_address, None)
                }
            }
        }
    }

    fn get_file_path(
        &self,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
        unit: &gimli::Unit<EndianSlice<RunTimeEndian>>,
        header: &gimli::LineProgramHeader<EndianSlice<RunTimeEndian>>,
        file_index: u64,
    ) -> Option<PathBuf> {
        let file = header.file(file_index)?;
        let mut path = PathBuf::new();

        if let Some(directory) = file.directory(header) {
            let dir_attr = dwarf.attr_string(unit, directory).ok()?;
            path.push(dir_attr.to_string_lossy().as_ref());
        }

        let file_name_attr = dwarf.attr_string(unit, file.path_name()).ok()?;
        path.push(file_name_attr.to_string_lossy().as_ref());

        Some(path)
    }

    /// Lookup source location for a given WASM offset
    pub fn lookup(&self, offset: usize) -> Option<SourceLocation> {
        // Find the last entry with Key <= offset using BTreeMap
        self.offsets
            .range(..=offset)
            .next_back()
            .map(|(_, loc)| loc.clone())
    }

    /// (Internal/Test) Manually add a mapping
    pub fn add_mapping(&mut self, offset: usize, loc: SourceLocation) {
        self.offsets.insert(offset, loc);
    }

    /// Get source code line for a given location
    pub fn get_source_line(&mut self, location: &SourceLocation) -> Option<String> {
        let content = self.get_source_content(&location.file)?;
        content
            .lines()
            .nth(location.line.saturating_sub(1) as usize)
            .map(|s| s.to_string())
    }

    /// Get full source content, with caching
    pub fn get_source_content(&mut self, path: &Path) -> Option<&str> {
        if !self.source_cache.contains_key(path) {
            if let Ok(content) = fs::read_to_string(path) {
                self.source_cache.insert(path.to_path_buf(), content);
            } else {
                return None;
            }
        }
        self.source_cache.get(path).map(|s| s.as_str())
    }

    /// Clear the source cache
    pub fn clear_cache(&mut self) {
        self.source_cache.clear();
    }

    /// How many full DWARF parses have been performed on this instance.
    ///
    /// Each call to `load()` that finds a **different** WASM (hash mismatch or
    /// first load) increments this counter.  Repeated loads with identical bytes
    /// are cache hits and do **not** increment the counter.  Useful in tests to
    /// assert that caching is working.
    pub fn parse_count(&self) -> usize {
        self.parse_count
    }

    /// The FNV-1a hash of the WASM bytes from the most recent parse, if any.
    ///
    /// `None` means no successful parse has been performed yet on this instance.
    pub fn last_wasm_hash(&self) -> Option<u64> {
        self.last_wasm_hash
    }

    /// Explicitly invalidate the parse cache.  The next call to `load()` will
    /// re-parse DWARF even if the bytes are identical to the previous load.
    pub fn invalidate_cache(&mut self) {
        self.last_wasm_hash = None;
    }

    /// Checks if the given exported function name has any source mappings.
    pub fn function_has_source_mapped(&self, wasm_bytes: &[u8], exported_function: &str) -> bool {
        let Ok(wasm_index) = WasmIndex::parse(wasm_bytes) else {
            return false;
        };
        let Some(func_idx) = wasm_index.function_index_for_export(exported_function) else {
            return false;
        };
        let bodies = &wasm_index.function_bodies;
        let Some((range, _)) = bodies.iter().find(|(_, idx)| *idx == func_idx) else {
            return false;
        };
        self.offsets.range(range.clone()).next().is_some()
    }

    /// Resolve source breakpoints for a source file into exact executable lines using DWARF line mappings.
    ///
    /// This relies on:
    /// - DWARF line program mappings (already loaded into this `SourceMap`)
    pub fn resolve_source_breakpoints(
        &self,
        _wasm_bytes: &[u8],
        source_path: &Path,
        requested_lines: &[u32],
        max_forward: Option<u32>,
    ) -> Vec<SourceBreakpointResolution> {
        let max_forward = max_forward.unwrap_or(20);

        if requested_lines.is_empty() {
            return Vec::new();
        }

        if self.is_empty() {
            return requested_lines
                .iter()
                .map(|line| SourceBreakpointResolution {
                    requested_line: *line,
                    line: *line,
                    verified: false,
                    function: None,
                    reason_code: "NO_DEBUG_INFO".to_string(),
                    message: "[NO_DEBUG_INFO] Contract is missing DWARF source mappings; rebuild with debug info to bind source breakpoints accurately.".to_string(),
                })
                .collect();
        }

        let requested_norm = normalize_path_for_match(source_path);
        let filename_ambiguous = is_filename_ambiguous(
            self.offsets
                .values()
                .map(|loc| normalize_path_for_match(&loc.file)),
            &requested_norm,
        );
        let mut line_to_offsets: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        let mut file_match_count = 0usize;

        // Build a file-specific line->offset index.
        for (offset, loc) in self.offsets.iter() {
            if loc.line == 0 {
                continue;
            }

            if !paths_match_normalized(
                &normalize_path_for_match(&loc.file),
                &requested_norm,
                filename_ambiguous,
            ) {
                continue;
            }

            file_match_count += 1;
            line_to_offsets.entry(loc.line).or_default().push(*offset);
        }

        if file_match_count == 0 {
            return requested_lines
                .iter()
                .map(|line| SourceBreakpointResolution {
                    requested_line: *line,
                    line: *line,
                    verified: false,
                    function: None,
                    reason_code: "FILE_NOT_IN_DEBUG_INFO".to_string(),
                    message: format!(
                        "[FILE_NOT_IN_DEBUG_INFO] Source file '{}' is not present in contract debug info (DWARF).",
                        source_path.to_string_lossy()
                    ),
                })
                .collect();
        }

        requested_lines
            .iter()
            .map(|requested_line| {
                let mut resolved_line = *requested_line;
                let mut adjusted = false;

                let _offsets = if let Some(offsets) = line_to_offsets.get(requested_line) {
                    offsets.as_slice()
                } else {
                    let mut found: Option<(u32, &Vec<usize>)> = None;
                    if let Some((next_line, offsets)) =
                        line_to_offsets.range(*requested_line..).next()
                    {
                        if next_line.saturating_sub(*requested_line) <= max_forward {
                            found = Some((*next_line, offsets));
                        }
                    }

                    if let Some((next_line, offsets)) = found {
                        adjusted = true;
                        resolved_line = next_line;
                        offsets.as_slice()
                    } else {
                        return SourceBreakpointResolution {
                            requested_line: *requested_line,
                            line: *requested_line,
                            verified: false,
                            function: None,
                            reason_code: "NO_CODE_AT_LINE".to_string(),
                            message: "[NO_CODE_AT_LINE] No executable code found at or near this line in contract debug info.".to_string(),
                        };
                    }
                };

                let location = format!("{}:{}", source_path.to_string_lossy(), resolved_line);

                SourceBreakpointResolution {
                    requested_line: *requested_line,
                    line: resolved_line,
                    verified: true,
                    function: Some(location),
                    reason_code: if adjusted {
                        "ADJUSTED".to_string()
                    } else {
                        "OK".to_string()
                    },
                    message: if adjusted {
                        format!("Adjusted to executable line {}.", resolved_line)
                    } else {
                        "Mapped to exact source line.".to_string()
                    },
                }
            })
            .collect()
    }
}

fn dwarf_section_sizes(wasm_bytes: &[u8]) -> Result<HashMap<String, usize>> {
    let mut sections = HashMap::new();
    for payload in Parser::new(0).parse_all(wasm_bytes) {
        let payload = payload
            .map_err(|e| DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e)))?;
        if let Payload::CustomSection(reader) = payload {
            let name = reader.name().to_string();
            if DWARF_SECTION_NAMES
                .iter()
                .any(|known| *known == name || known.trim_start_matches('.') == name)
            {
                let normalized = if name.starts_with('.') {
                    name
                } else {
                    format!(".{}", name)
                };
                sections.insert(normalized, reader.data().len());
            }
        }
    }
    Ok(sections)
}

#[derive(Debug, Clone)]
struct WasmIndex {
    function_bodies: Vec<(std::ops::Range<usize>, u32)>,
    function_by_export: HashMap<String, u32>,
}

impl WasmIndex {
    fn parse(wasm_bytes: &[u8]) -> Result<Self> {
        let mut imported_func_count = 0u32;
        let mut local_function_index = 0u32;
        let mut function_bodies: Vec<(std::ops::Range<usize>, u32)> = Vec::new();
        let mut function_by_export: HashMap<String, u32> = HashMap::new();

        for payload in Parser::new(0).parse_all(wasm_bytes) {
            let payload = payload.map_err(|e| {
                DebuggerError::WasmLoadError(format!("Failed to parse WASM: {}", e))
            })?;

            match payload {
                Payload::ImportSection(reader) => {
                    for import in reader {
                        let import = import.map_err(|e| {
                            DebuggerError::WasmLoadError(format!("Failed to read import: {}", e))
                        })?;
                        if matches!(import.ty, wasmparser::TypeRef::Func(_)) {
                            imported_func_count = imported_func_count.saturating_add(1);
                        }
                    }
                }
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export.map_err(|e| {
                            DebuggerError::WasmLoadError(format!("Failed to read export: {}", e))
                        })?;
                        if matches!(export.kind, wasmparser::ExternalKind::Func) {
                            let func_index = export.index;
                            // Prefer first name if multiple exports point at same index.
                            function_by_export
                                .entry(export.name.to_string())
                                .or_insert(func_index);
                        }
                    }
                }
                Payload::CodeSectionEntry(reader) => {
                    let function_index = imported_func_count.saturating_add(local_function_index);
                    local_function_index = local_function_index.saturating_add(1);
                    function_bodies.push((reader.range(), function_index));
                }
                _ => {}
            }
        }

        // WASM parser yields code entries in module order; sort by start for binary search safety.
        function_bodies.sort_by_key(|(range, _)| range.start);

        Ok(Self {
            function_bodies,
            function_by_export,
        })
    }

    fn function_index_for_export(&self, export_name: &str) -> Option<u32> {
        self.function_by_export.get(export_name).copied()
    }
}

fn normalize_path_for_match(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim()
        .to_ascii_lowercase()
}

fn paths_match_normalized(a: &str, b: &str, filename_ambiguous: bool) -> bool {
    if a == b {
        return true;
    }

    if a.ends_with(b) || b.ends_with(a) {
        return true;
    }

    let a_file = a.rsplit('/').next().unwrap_or(a);
    let b_file = b.rsplit('/').next().unwrap_or(b);
    if a_file != b_file {
        return false;
    }

    if !filename_ambiguous {
        return true;
    }

    let a_components = a.split('/').filter(|part| !part.is_empty()).count();
    let b_components = b.split('/').filter(|part| !part.is_empty()).count();
    let components_to_match = a_components.min(b_components);

    if components_to_match < 2 {
        return false;
    }

    suffix_components_match(a, b, components_to_match)
}

fn suffix_components_match(a: &str, b: &str, components: usize) -> bool {
    let a_parts: Vec<&str> = a.split('/').filter(|part| !part.is_empty()).collect();
    let b_parts: Vec<&str> = b.split('/').filter(|part| !part.is_empty()).collect();
    if a_parts.len() < components || b_parts.len() < components {
        return false;
    }

    a_parts[a_parts.len() - components..] == b_parts[b_parts.len() - components..]
}

fn is_filename_ambiguous<I>(paths: I, requested: &str) -> bool
where
    I: Iterator<Item = String>,
{
    let requested_file = requested.rsplit('/').next().unwrap_or(requested);
    let mut matching_paths: HashSet<String> = HashSet::new();

    for path in paths {
        let file = path.rsplit('/').next().unwrap_or(&path);
        if file == requested_file {
            matching_paths.insert(path);
            if matching_paths.len() > 1 {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fnv1a_hash ───────────────────────────────────────────────────────────

    #[test]
    fn hash_is_deterministic() {
        let data = b"hello world";
        assert_eq!(fnv1a_hash(data), fnv1a_hash(data));
    }

    #[test]
    fn hash_differs_for_different_inputs() {
        assert_ne!(fnv1a_hash(b"aaa"), fnv1a_hash(b"bbb"));
    }

    #[test]
    fn hash_of_empty_is_the_offset_basis() {
        assert_eq!(fnv1a_hash(&[]), 0xcbf2_9ce4_8422_2325);
    }

    // ── SourceMap cache behaviour ────────────────────────────────────────────

    /// Minimal valid WASM (no DWARF) — lets us call `load()` without a real
    /// contract fixture on disk.
    fn tiny_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d, // magic
            0x01, 0x00, 0x00, 0x00, // version
            0x01, 0x01, 0x00, // type section: size 1, count 0
        ]
    }

    fn uleb128(mut value: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }

    fn wasm_with_custom_section(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        bytes.push(0x00);

        let mut section = Vec::new();
        section.extend_from_slice(&uleb128(name.len()));
        section.extend_from_slice(name.as_bytes());
        section.extend_from_slice(payload);

        bytes.extend_from_slice(&uleb128(section.len()));
        bytes.extend_from_slice(&section);
        bytes
    }

    #[test]
    fn normalize_wasm_offsets_keeps_alternate_for_ambiguous_addresses() {
        let sm = SourceMap {
            code_section_range: Some(100..400),
            ..SourceMap::new()
        };

        let (primary, alternate) = sm.normalize_wasm_offsets(150, 500);

        assert_eq!(primary, 150);
        assert_eq!(alternate, Some(250));
    }

    #[test]
    fn ambiguous_addresses_resolve_for_both_coordinate_spaces() {
        let mut sm = SourceMap {
            code_section_range: Some(100..400),
            ..SourceMap::new()
        };
        let location = SourceLocation {
            file: PathBuf::from("lib.rs"),
            line: 9,
            column: Some(1),
        };

        let (primary, alternate) = sm.normalize_wasm_offsets(150, 500);
        sm.offsets.insert(primary, location.clone());
        if let Some(alternate) = alternate {
            sm.offsets.entry(alternate).or_insert(location.clone());
        }

        assert_eq!(sm.lookup(150), Some(location.clone()));
        assert_eq!(sm.lookup(250), Some(location));
    }

    #[test]
    fn first_load_increments_parse_count() {
        let mut sm = SourceMap::new();
        assert_eq!(sm.parse_count(), 0);
        sm.load(&tiny_wasm()).unwrap();
        assert_eq!(sm.parse_count(), 1);
    }

    #[test]
    fn repeated_load_with_same_bytes_does_not_re_parse() {
        let bytes = tiny_wasm();
        let mut sm = SourceMap::new();
        sm.load(&bytes).unwrap();
        sm.load(&bytes).unwrap();
        sm.load(&bytes).unwrap();
        assert_eq!(sm.parse_count(), 1, "only the first call should parse");
    }

    #[test]
    fn inspection_report_lists_missing_dwarf_sections_and_wasm_fallback() {
        let report = SourceMap::inspect_wasm(&tiny_wasm(), 5).unwrap();

        assert_eq!(report.mappings_count, 0);
        assert_eq!(report.fallback_mode, "wasm-only");
        assert!(report
            .sections
            .iter()
            .any(|section| section.name == ".debug_info" && !section.present));
        assert!(report.fallback_message.contains("Missing DWARF sections"));
    }

    #[test]
    fn inspection_report_marks_present_dwarf_sections() {
        let wasm = wasm_with_custom_section(".debug_info", &[1, 2, 3, 4]);
        let report = SourceMap::inspect_wasm(&wasm, 5).unwrap();

        assert!(report
            .sections
            .iter()
            .any(|section| section.name == ".debug_info"
                && section.present
                && section.size_bytes == 4));
    }

    #[test]
    fn load_with_different_bytes_re_parses() {
        let bytes_a = tiny_wasm();
        // A valid minimal WASM with no sections (just magic + version) — distinct from tiny_wasm().
        let bytes_b = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

        let mut sm = SourceMap::new();
        sm.load(&bytes_a).unwrap();
        sm.load(&bytes_b).unwrap();
        assert_eq!(sm.parse_count(), 2);
    }

    #[test]
    fn last_wasm_hash_is_none_before_first_load() {
        assert_eq!(SourceMap::new().last_wasm_hash(), None);
    }

    #[test]
    fn last_wasm_hash_matches_loaded_bytes() {
        let bytes = tiny_wasm();
        let mut sm = SourceMap::new();
        sm.load(&bytes).unwrap();
        assert_eq!(sm.last_wasm_hash(), Some(fnv1a_hash(&bytes)));
    }

    #[test]
    fn invalidate_cache_forces_re_parse() {
        let bytes = tiny_wasm();
        let mut sm = SourceMap::new();
        sm.load(&bytes).unwrap();
        assert_eq!(sm.parse_count(), 1);

        sm.invalidate_cache();
        sm.load(&bytes).unwrap();
        assert_eq!(
            sm.parse_count(),
            2,
            "re-parse must occur after explicit invalidation"
        );
    }

    #[test]
    fn cache_hit_preserves_manually_added_mappings() {
        let bytes = tiny_wasm();
        let mut sm = SourceMap::new();

        // Prime the cache with one real parse.
        sm.load(&bytes).unwrap();
        assert_eq!(sm.parse_count(), 1);

        // Add a mapping after the parse — simulates an externally injected entry.
        sm.add_mapping(
            10,
            SourceLocation {
                file: PathBuf::from("lib.rs"),
                line: 2,
                column: None,
            },
        );

        // Cache-hit load must not wipe the mapping.
        sm.load(&bytes).unwrap();
        assert_eq!(sm.parse_count(), 1);
        assert!(
            sm.lookup(10).is_some(),
            "manually added mapping must survive a cache-hit load"
        );
    }

    #[test]
    fn cache_miss_clears_stale_mappings() {
        let bytes_a = tiny_wasm();
        // Valid minimal WASM (magic+version only) — distinct from tiny_wasm().
        let bytes_b = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

        let mut sm = SourceMap::new();
        sm.load(&bytes_a).unwrap();
        sm.add_mapping(
            42,
            SourceLocation {
                file: PathBuf::from("old.rs"),
                line: 1,
                column: None,
            },
        );

        // Loading different bytes is a cache miss → stale mappings must be cleared.
        sm.load(&bytes_b).unwrap();
        assert_eq!(sm.parse_count(), 2);
        assert!(
            sm.lookup(42).is_none(),
            "stale mappings must be cleared on cache miss"
        );
    }
}
