use crate::Result;
use wasmparser::{Parser, Payload};

/// Parse exported functions from WASM
pub fn parse_functions(wasm_bytes: &[u8]) -> Result<Vec<String>> {
    let mut functions = Vec::new();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        if let Payload::ExportSection(reader) = payload? {
            for export in reader {
                let export = export?;
                if matches!(export.kind, wasmparser::ExternalKind::Func) {
                    functions.push(export.name.to_string());
                }
            }
        }
    }

    Ok(functions)
}

/// Get WASM module information
pub fn get_module_info(wasm_bytes: &[u8]) -> Result<ModuleInfo> {
    let mut info = ModuleInfo::default();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        match payload? {
            Payload::Version { .. } => {}
            Payload::TypeSection(reader) => {
                info.type_count = reader.count();
            }
            Payload::FunctionSection(reader) => {
                info.function_count = reader.count();
            }
            Payload::ExportSection(reader) => {
                info.export_count = reader.count();
            }
            _ => {}
        }
    }

    Ok(info)
}

/// Information about a WASM module
#[derive(Debug, Default)]
pub struct ModuleInfo {
    pub type_count: u32,
    pub function_count: u32,
    pub export_count: u32,
}
