pub mod source_map;
pub mod wasm;

pub use source_map::{SourceLocation, SourceMap};
pub use wasm::{get_module_info, parse_functions, ModuleInfo};
