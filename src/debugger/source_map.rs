use anyhow::{Context, Result};
use gimli::{Dwarf, EndianSlice, RunTimeEndian};
use object::{Object, ObjectSection};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

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
}

impl SourceMap {
    /// Create a new empty source map
    pub fn new() -> Self {
        Self {
            offsets: BTreeMap::new(),
            source_cache: HashMap::new(),
        }
    }

    /// Load debug info from WASM bytes and build the mapping
    pub fn load(&mut self, wasm_bytes: &[u8]) -> Result<()> {
        let obj = object::File::parse(wasm_bytes).context("Failed to parse WASM object file")?;

        let load_section =
            |id: gimli::SectionId| -> Result<EndianSlice<RunTimeEndian>, gimli::Error> {
                let section = obj.section_by_name(id.name()).unwrap_or_else(|| {
                    obj.section_by_name(&format!(".{}", id.name()))
                        .unwrap_or_else(|| object::Section {
                            index: object::SectionIndex(0),
                            id: object::SectionId(0),
                            name: id.name(),
                            segment: None,
                            address: 0,
                            size: 0,
                            align: 0,
                            data_range: None,
                            data: &[],
                            relocations: Vec::new(),
                            flags: object::SectionFlags::None,
                            symbol_index: None,
                            kind: object::SectionKind::Other,
                        })
                });
                Ok(EndianSlice::new(
                    section.data().unwrap_or(&[]),
                    RunTimeEndian::Little,
                ))
            };

        let dwarf = Dwarf::load(&load_section).context("Failed to load DWARF sections")?;

        let mut units = dwarf.units();
        while let Some(header) = units.next()? {
            let unit = dwarf.unit(header)?;
            if let Some(program) = unit.line_program.clone() {
                let mut rows = program.rows();
                while let Some((header, row)) = rows.next_row()? {
                    if let Some(file_path) =
                        self.get_file_path(&dwarf, &unit, header, row.file_index())
                    {
                        // In WASM, DWARF addresses are usually offsets into the code section
                        let offset = row.address() as usize;
                        let line = row.line().map(|l| l.get() as u32).unwrap_or(0);
                        let column = row.column().column().map(|c| c.get() as u32);

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
}
