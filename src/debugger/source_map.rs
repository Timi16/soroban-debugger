use crate::{DebuggerError, Result};
use gimli::{Dwarf, EndianSlice, RunTimeEndian};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use wasmparser::{Parser, Payload};

/// Represents a source code location
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
}

/// Manages mapping from WASM offsets to source code locations
pub struct SourceMap {
    /// Mapping from offset to source location (sorted by offset)
    offsets: BTreeMap<usize, SourceLocation>,
    /// Cache of source file contents
    source_cache: HashMap<PathBuf, String>,
    /// Code section payload range (when known), used to normalize DWARF addresses.
    code_section_range: Option<std::ops::Range<usize>>,
}

/// Result of resolving a source breakpoint (file + line) to a concrete contract entrypoint breakpoint.
///
/// The debugger currently supports function-level breakpoints, so source breakpoints resolve to a
/// single exported function name (entrypoint) when possible.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceBreakpointResolution {
    /// The requested 1-based source line.
    pub requested_line: u32,
    /// The resolved 1-based source line (may be adjusted to the next executable line).
    pub line: u32,
    /// Whether the breakpoint binding is considered exact/high-confidence.
    pub verified: bool,
    /// Exported function (entrypoint) name to bind a runtime breakpoint to.
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
        }
    }

    /// Load debug info from WASM bytes and build the mapping
    pub fn load(&mut self, wasm_bytes: &[u8]) -> Result<()> {
        self.offsets.clear();
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

        let dwarf = Dwarf::load(&load_section).map_err(|e| {
            DebuggerError::WasmLoadError(format!("Failed to load DWARF sections: {}", e))
        })?;

        let mut units = dwarf.units();
        while let Some(header) = units.next().map_err(|e| {
            DebuggerError::WasmLoadError(format!("Failed to read DWARF unit: {}", e))
        })? {
            let unit = dwarf.unit(header).map_err(|e| {
                DebuggerError::WasmLoadError(format!("Failed to load DWARF unit: {}", e))
            })?;
            if let Some(program) = unit.line_program.clone() {
                let mut rows = program.rows();
                while let Some((header, row)) = rows.next_row().map_err(|e| {
                    DebuggerError::WasmLoadError(format!("Failed to read DWARF line row: {}", e))
                })? {
                    if let Some(file_path) =
                        self.get_file_path(&dwarf, &unit, header, row.file_index())
                    {
                        // In WASM, DWARF addresses are usually offsets into the code section
                        let offset =
                            self.normalize_wasm_offset(row.address() as usize, wasm_bytes.len());
                        let line = row.line().map(|l| l.get() as u32).unwrap_or(0);
                        let column = match row.column() {
                            gimli::ColumnType::LeftEdge => None,
                            gimli::ColumnType::Column(column) => Some(column.get() as u32),
                        };

                        self.offsets.insert(
                            offset,
                            SourceLocation {
                                file: file_path,
                                line,
                                column,
                            },
                        );
                    }
                }
            }
        }

        Ok(())
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

    fn normalize_wasm_offset(&self, dwarf_address: usize, wasm_len: usize) -> usize {
        let Some(code_range) = &self.code_section_range else {
            return dwarf_address;
        };

        // Common case: DWARF line-program addresses are offsets into the code-section payload.
        let code_start = code_range.start;
        let code_len = code_range.end.saturating_sub(code_range.start);

        // If the address already looks like a module/file offset, keep it.
        if dwarf_address >= code_start && dwarf_address < wasm_len {
            return dwarf_address;
        }

        // Otherwise, treat addresses within the code-section payload length as relative.
        if dwarf_address < code_len {
            return code_start.saturating_add(dwarf_address);
        }

        dwarf_address
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

    /// Resolve source breakpoints for a source file into exported contract functions using DWARF line mappings.
    ///
    /// This relies on:
    /// - DWARF line program mappings (already loaded into this `SourceMap`)
    /// - WASM code section entry ranges (offset -> function index)
    /// - WASM export section (function index -> exported names)
    /// - The provided `exported_functions` allowlist, usually derived from `inspect --functions`.
    pub fn resolve_source_breakpoints(
        &self,
        wasm_bytes: &[u8],
        source_path: &Path,
        requested_lines: &[u32],
        exported_functions: &HashSet<String>,
    ) -> Vec<SourceBreakpointResolution> {
        const MAX_FORWARD_LINE_ADJUST: u32 = 20;

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

        let wasm_index = match WasmIndex::parse(wasm_bytes) {
            Ok(index) => index,
            Err(e) => {
                return requested_lines
                    .iter()
                    .map(|line| SourceBreakpointResolution {
                        requested_line: *line,
                        line: *line,
                        verified: false,
                        function: None,
                        reason_code: "WASM_PARSE_ERROR".to_string(),
                        message: format!(
                            "[WASM_PARSE_ERROR] Failed to parse WASM for breakpoint resolution: {}",
                            e
                        ),
                    })
                    .collect();
            }
        };

        let requested_norm = normalize_path_for_match(source_path);
        let mut line_to_offsets: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        let mut file_match_count = 0usize;

        // Build a file-specific line->offset index.
        for (offset, loc) in self.offsets.iter() {
            if loc.line == 0 {
                continue;
            }

            if !paths_match_normalized(&normalize_path_for_match(&loc.file), &requested_norm) {
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

        // Pre-compute per-function line spans for this file (for disambiguation).
        let mut function_spans: HashMap<u32, (u32, u32)> = HashMap::new();
        for (line, offsets) in line_to_offsets.iter() {
            for offset in offsets {
                if let Some(function_index) = wasm_index.function_index_for_offset(*offset) {
                    let entry = function_spans
                        .entry(function_index)
                        .or_insert((*line, *line));
                    entry.0 = entry.0.min(*line);
                    entry.1 = entry.1.max(*line);
                }
            }
        }

        requested_lines
            .iter()
            .map(|requested_line| {
                let mut resolved_line = *requested_line;
                let mut adjusted = false;

                let offsets = if let Some(offsets) = line_to_offsets.get(requested_line) {
                    offsets.as_slice()
                } else {
                    let mut found: Option<(u32, &Vec<usize>)> = None;
                    if let Some((next_line, offsets)) =
                        line_to_offsets.range(*requested_line..).next()
                    {
                        if next_line.saturating_sub(*requested_line) <= MAX_FORWARD_LINE_ADJUST {
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

                let mut candidate_entrypoints: HashSet<String> = HashSet::new();
                let mut non_exported_function_indices: HashSet<u32> = HashSet::new();

                for offset in offsets {
                    let Some(function_index) = wasm_index.function_index_for_offset(*offset) else {
                        continue;
                    };

                    let Some(export_names) = wasm_index.export_names_for_function(function_index)
                    else {
                        non_exported_function_indices.insert(function_index);
                        continue;
                    };

                    let mut any_allowed = false;
                    for name in export_names {
                        if exported_functions.contains(name) {
                            any_allowed = true;
                            candidate_entrypoints.insert(name.clone());
                        }
                    }

                    if !any_allowed {
                        non_exported_function_indices.insert(function_index);
                    }
                }

                if candidate_entrypoints.is_empty() {
                    if !non_exported_function_indices.is_empty() {
                        let mut indices: Vec<u32> = non_exported_function_indices.into_iter().collect();
                        indices.sort_unstable();
                        indices.truncate(5);
                        return SourceBreakpointResolution {
                            requested_line: *requested_line,
                            line: resolved_line,
                            verified: false,
                            function: None,
                            reason_code: "NOT_EXPORTED".to_string(),
                            message: format!(
                                "[NOT_EXPORTED] Line maps to non-entrypoint WASM function(s) {:?}; only exported contract entrypoints can be targeted.",
                                indices
                            ),
                        };
                    }

                    return SourceBreakpointResolution {
                        requested_line: *requested_line,
                        line: resolved_line,
                        verified: false,
                        function: None,
                        reason_code: "UNMAPPABLE".to_string(),
                        message: "[UNMAPPABLE] Unable to map line to an exported contract entrypoint.".to_string(),
                    };
                }

                let mut candidates: Vec<String> = candidate_entrypoints.into_iter().collect();
                candidates.sort();

                let chosen = if candidates.len() == 1 {
                    Some(candidates[0].clone())
                } else {
                    // Disambiguate using per-function line spans within this file.
                    let mut matching: Vec<String> = Vec::new();
                    for candidate in candidates.iter() {
                        if let Some(function_index) =
                            wasm_index.function_index_for_export(candidate)
                        {
                            if let Some((min_line, max_line)) = function_spans.get(&function_index)
                            {
                                if *requested_line >= *min_line && *requested_line <= *max_line {
                                    matching.push(candidate.clone());
                                }
                            }
                        }
                    }

                    if matching.len() == 1 {
                        Some(matching.remove(0))
                    } else {
                        None
                    }
                };

                let Some(function) = chosen else {
                    return SourceBreakpointResolution {
                        requested_line: *requested_line,
                        line: resolved_line,
                        verified: false,
                        function: None,
                        reason_code: "AMBIGUOUS".to_string(),
                        message: format!(
                            "[AMBIGUOUS] Source line could map to multiple entrypoints {:?}.",
                            candidates
                        ),
                    };
                };

                SourceBreakpointResolution {
                    requested_line: *requested_line,
                    line: resolved_line,
                    verified: true,
                    function: Some(function.clone()),
                    reason_code: if adjusted {
                        "ADJUSTED".to_string()
                    } else {
                        "OK".to_string()
                    },
                    message: if adjusted {
                        format!("Adjusted to line {} and mapped to entrypoint '{}'.", resolved_line, function)
                    } else {
                        format!("Mapped to entrypoint '{}'.", function)
                    },
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct WasmIndex {
    function_bodies: Vec<(std::ops::Range<usize>, u32)>,
    exports_by_function: HashMap<u32, Vec<String>>,
    function_by_export: HashMap<String, u32>,
}

impl WasmIndex {
    fn parse(wasm_bytes: &[u8]) -> Result<Self> {
        let mut imported_func_count = 0u32;
        let mut local_function_index = 0u32;
        let mut function_bodies: Vec<(std::ops::Range<usize>, u32)> = Vec::new();
        let mut exports_by_function: HashMap<u32, Vec<String>> = HashMap::new();
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
                            exports_by_function
                                .entry(func_index)
                                .or_default()
                                .push(export.name.to_string());
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
            exports_by_function,
            function_by_export,
        })
    }

    fn function_index_for_export(&self, export_name: &str) -> Option<u32> {
        self.function_by_export.get(export_name).copied()
    }

    fn export_names_for_function(&self, function_index: u32) -> Option<&Vec<String>> {
        self.exports_by_function.get(&function_index)
    }

    fn function_index_for_offset(&self, offset: usize) -> Option<u32> {
        let bodies = self.function_bodies.as_slice();
        if bodies.is_empty() {
            return None;
        }

        // Find rightmost body with start <= offset.
        let idx = match bodies.binary_search_by_key(&offset, |(range, _)| range.start) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };

        let (range, function_index) = &bodies[idx];
        if offset >= range.start && offset < range.end {
            Some(*function_index)
        } else {
            None
        }
    }
}

fn normalize_path_for_match(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim()
        .to_ascii_lowercase()
}

fn paths_match_normalized(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }

    if a.ends_with(b) || b.ends_with(a) {
        return true;
    }

    let a_file = a.rsplit('/').next().unwrap_or(a);
    let b_file = b.rsplit('/').next().unwrap_or(b);
    a_file == b_file
}
