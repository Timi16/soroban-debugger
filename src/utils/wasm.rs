use crate::Result;
use serde::Deserialize;
use wasmparser::{Parser, Payload};

// ─── existing public API (unchanged) ─────────────────────────────────────────

/// Parse exported functions from a WASM module.
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

/// Get high-level module statistics from a WASM binary.
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

/// Information about a WASM module.
#[derive(Debug, Default)]
pub struct ModuleInfo {
    pub type_count: u32,
    pub function_count: u32,
    pub export_count: u32,
}

// ─── metadata types ───────────────────────────────────────────────────────────

/// High-level contract metadata extracted from WASM custom sections.
///
/// All fields are optional; missing values are handled gracefully.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContractMetadata {
    pub contract_version: Option<String>,
    pub sdk_version: Option<String>,
    pub build_date: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub implementation: Option<String>,
}

impl ContractMetadata {
    /// Returns `true` when no metadata fields have been populated.
    pub fn is_empty(&self) -> bool {
        self.contract_version.is_none()
            && self.sdk_version.is_none()
            && self.build_date.is_none()
            && self.author.is_none()
            && self.description.is_none()
            && self.implementation.is_none()
    }
}

/// Serde-compatible intermediate type for parsing JSON metadata payloads.
///
/// Both snake_case and camelCase field names are accepted for flexibility.
#[derive(Debug, Default, Deserialize)]
struct JsonContractMetadata {
    #[serde(alias = "contract_version", alias = "contractVersion")]
    contract_version: Option<String>,

    #[serde(alias = "sdk_version", alias = "sdkVersion")]
    sdk_version: Option<String>,

    #[serde(alias = "build_date", alias = "buildDate")]
    build_date: Option<String>,

    #[serde(alias = "author", alias = "organisation", alias = "organization")]
    author: Option<String>,

    #[serde(alias = "description")]
    description: Option<String>,

    #[serde(
        alias = "implementation",
        alias = "implementation_notes",
        alias = "implementationNotes"
    )]
    implementation: Option<String>,
}

impl From<JsonContractMetadata> for ContractMetadata {
    fn from(j: JsonContractMetadata) -> Self {
        ContractMetadata {
            contract_version: j.contract_version,
            sdk_version: j.sdk_version,
            build_date: j.build_date,
            author: j.author,
            description: j.description,
            implementation: j.implementation,
        }
    }
}

// ─── metadata extraction ──────────────────────────────────────────────────────

/// Extract contract metadata from WASM custom sections.
///
/// Searches for a `contractmeta` custom section containing UTF-8 text.  The
/// payload is first interpreted as JSON; if that fails, a permissive
/// `key: value` / `key=value` line-based format is attempted.
///
/// Contracts that embed no metadata return an empty [`ContractMetadata`]
/// without error.
pub fn extract_contract_metadata(wasm_bytes: &[u8]) -> Result<ContractMetadata> {
    let mut metadata = ContractMetadata::default();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        let Payload::CustomSection(reader) = payload? else {
            continue;
        };

        if reader.name() != "contractmeta" {
            continue;
        }

        let data = reader.data();
        let Ok(text) = std::str::from_utf8(data) else {
            // Non-UTF-8 custom section data is skipped silently.
            continue;
        };

        // ── attempt JSON deserialization first ────────────────────────────
        if let Ok(json_meta) = serde_json::from_str::<JsonContractMetadata>(text) {
            let parsed: ContractMetadata = json_meta.into();

            if metadata.contract_version.is_none() {
                metadata.contract_version = parsed.contract_version;
            }
            if metadata.sdk_version.is_none() {
                metadata.sdk_version = parsed.sdk_version;
            }
            if metadata.build_date.is_none() {
                metadata.build_date = parsed.build_date;
            }
            if metadata.author.is_none() {
                metadata.author = parsed.author;
            }
            if metadata.description.is_none() {
                metadata.description = parsed.description;
            }
            if metadata.implementation.is_none() {
                metadata.implementation = parsed.implementation;
            }

            if !metadata.is_empty() {
                break;
            }

            continue;
        }

        // ── fallback: "key: value" / "key=value" line-based format ────────
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let (key, value) = if let Some((k, v)) = line.split_once('=') {
                (k.trim(), v.trim())
            } else if let Some((k, v)) = line.split_once(':') {
                (k.trim(), v.trim())
            } else {
                continue;
            };

            match key {
                "contract_version" | "contractVersion" if metadata.contract_version.is_none() => {
                    metadata.contract_version = Some(value.to_string());
                }
                "sdk_version" | "sdkVersion" if metadata.sdk_version.is_none() => {
                    metadata.sdk_version = Some(value.to_string());
                }
                "build_date" | "buildDate" if metadata.build_date.is_none() => {
                    metadata.build_date = Some(value.to_string());
                }
                "author" | "organisation" | "organization" if metadata.author.is_none() => {
                    metadata.author = Some(value.to_string());
                }
                "description" if metadata.description.is_none() => {
                    metadata.description = Some(value.to_string());
                }
                "implementation" | "implementation_notes" | "implementationNotes"
                    if metadata.implementation.is_none() =>
                {
                    metadata.implementation = Some(value.to_string());
                }
                _ => {}
            }
        }
    }

    Ok(metadata)
}

// ─── contract spec / function signatures ─────────────────────────────────────

/// A single function parameter: name and its Soroban type as a display string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParam {
    pub name: String,
    pub type_name: String,
}

/// Full signature for one exported contract function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    pub name: String,
    pub params: Vec<FunctionParam>,
    pub return_type: Option<String>,
}

/// Convert an XDR `ScSpecTypeDef` into a human-readable type string.
fn spec_type_to_string(ty: &stellar_xdr::curr::ScSpecTypeDef) -> String {
    use stellar_xdr::curr::ScSpecTypeDef as T;
    match ty {
        T::Val       => "Val".into(),
        T::Bool      => "Bool".into(),
        T::Void      => "Void".into(),
        T::Error     => "Error".into(),
        T::U32       => "U32".into(),
        T::I32       => "I32".into(),
        T::U64       => "U64".into(),
        T::I64       => "I64".into(),
        T::Timepoint => "Timepoint".into(),
        T::Duration  => "Duration".into(),
        T::U128      => "U128".into(),
        T::I128      => "I128".into(),
        T::U256      => "U256".into(),
        T::I256      => "I256".into(),
        T::Bytes     => "Bytes".into(),
        T::String    => "String".into(),
        T::Symbol    => "Symbol".into(),
        T::Address   => "Address".into(),
        T::Option(o) => format!("Option<{}>", spec_type_to_string(&o.value_type)),
        T::Result(r) => format!(
            "Result<{}, {}>",
            spec_type_to_string(&r.ok_type),
            spec_type_to_string(&r.error_type),
        ),
        T::Vec(v)    => format!("Vec<{}>", spec_type_to_string(&v.element_type)),
        T::Map(m)    => format!(
            "Map<{}, {}>",
            spec_type_to_string(&m.key_type),
            spec_type_to_string(&m.value_type),
        ),
        T::Tuple(t)  => {
            let inner: Vec<String> = t.value_types.iter().map(spec_type_to_string).collect();
            format!("Tuple<{}>", inner.join(", "))
        }
        T::BytesN(b) => format!("BytesN<{}>", b.n),
        T::Udt(u)    => {
            std::str::from_utf8(u.name.as_slice())
                .unwrap_or("Udt")
                .to_string()
        }
    }
}

/// Helper: convert a `StringM<N>` slice to an owned `String` lossily.
fn stringm_to_string(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes)
        .unwrap_or("<invalid utf8>")
        .to_string()
}

/// Parse full function signatures from the WASM `contractspecv0` custom section.
///
/// Returns an empty `Vec` (not an error) when no spec section is present —
/// this keeps callers simple and backward-compatible with contracts that
/// pre-date the spec section.
pub fn parse_function_signatures(wasm_bytes: &[u8]) -> Result<Vec<FunctionSignature>> {
    use stellar_xdr::curr::{Limited, Limits, ReadXdr, ScSpecEntry};

    let mut signatures = Vec::new();
    let parser = Parser::new(0);

    for payload in parser.parse_all(wasm_bytes) {
        let Payload::CustomSection(reader) = payload? else {
            continue;
        };

        if reader.name() != "contractspecv0" {
            continue;
        }

        let data = reader.data();
        let cursor = std::io::Cursor::new(data);
        let mut limited = Limited::new(cursor, Limits::none());

        // The section is a packed sequence of XDR-encoded ScSpecEntry values.
        loop {
            match ScSpecEntry::read_xdr(&mut limited) {
                Ok(ScSpecEntry::FunctionV0(func)) => {
                    let name = stringm_to_string(func.name.0.as_slice());

                    let params = func
                        .inputs
                        .iter()
                        .map(|input| FunctionParam {
                            name:      stringm_to_string(input.name.as_slice()),
                            type_name: spec_type_to_string(&input.type_),
                        })
                        .collect();

                    let return_type = func
                        .outputs
                        .first()
                        .map(spec_type_to_string);

                    signatures.push(FunctionSignature { name, params, return_type });
                }
                Ok(_) => {
                    // UDT definitions, events, etc. — skip
                }
                Err(_) => break, // end of section or corrupt data
            }
        }

        break; // only one contractspecv0 section exists per contract
    }

    Ok(signatures)
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── WASM test-module builder ──────────────────────────────────────────────

    /// Encode `value` as an unsigned LEB128 byte sequence.
    ///
    /// WASM mandates LEB128 for all integer fields in the binary format,
    /// including section sizes and string lengths.  A plain `as u8` cast is
    /// only valid for values 0–127; anything larger requires multiple bytes.
    fn uleb128(mut value: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            // Take the 7 low-order bits.
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            // Set the continuation bit when more bytes follow.
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

    /// Build a minimal valid WASM module that contains a single custom section.
    ///
    /// Uses proper ULEB128 encoding so it works for payloads of any size,
    /// unlike a naïve single-byte length which panics above 127 bytes.
    fn make_custom_section_wasm(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();

        // WASM magic number and version.
        bytes.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]);
        bytes.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);

        // Section id 0 = custom section.
        bytes.push(0x00);

        // Section content: LEB128(name.len) ++ name ++ payload.
        let mut section = Vec::new();
        section.extend_from_slice(&uleb128(name.len()));
        section.extend_from_slice(name.as_bytes());
        section.extend_from_slice(payload);

        // Section size as LEB128, then the content.
        bytes.extend_from_slice(&uleb128(section.len()));
        bytes.extend_from_slice(&section);

        bytes
    }

    // ── metadata-present tests ────────────────────────────────────────────────

    #[test]
    fn extract_metadata_from_json_custom_section() {
        let json = r#"
        {
            "contract_version": "1.2.3",
            "sdk_version": "22.0.0",
            "build_date": "2026-02-20",
            "author": "Example Org",
            "description": "Sample contract for testing",
            "implementation_notes": "Uses JSON metadata format"
        }
        "#;

        let wasm = make_custom_section_wasm("contractmeta", json.as_bytes());
        let meta = extract_contract_metadata(&wasm).expect("metadata should parse");

        assert_eq!(meta.contract_version.as_deref(), Some("1.2.3"));
        assert_eq!(meta.sdk_version.as_deref(), Some("22.0.0"));
        assert_eq!(meta.build_date.as_deref(), Some("2026-02-20"));
        assert_eq!(meta.author.as_deref(), Some("Example Org"));
        assert_eq!(
            meta.description.as_deref(),
            Some("Sample contract for testing")
        );
        assert_eq!(
            meta.implementation.as_deref(),
            Some("Uses JSON metadata format")
        );
    }

    #[test]
    fn extract_metadata_from_line_based_custom_section() {
        let text = "\
contract_version: 0.0.1
sdkVersion=22.0.0
build_date: 2026-02-19
author=Example Dev
description: Line based metadata
implementation_notes=Line-based format
";

        let wasm = make_custom_section_wasm("contractmeta", text.as_bytes());
        let meta = extract_contract_metadata(&wasm).expect("metadata should parse");

        assert_eq!(meta.contract_version.as_deref(), Some("0.0.1"));
        assert_eq!(meta.sdk_version.as_deref(), Some("22.0.0"));
        assert_eq!(meta.build_date.as_deref(), Some("2026-02-19"));
        assert_eq!(meta.author.as_deref(), Some("Example Dev"));
        assert_eq!(meta.description.as_deref(), Some("Line based metadata"));
        assert_eq!(meta.implementation.as_deref(), Some("Line-based format"));
    }

    // ── metadata-absent tests ─────────────────────────────────────────────────

    #[test]
    fn extract_metadata_from_wasm_without_metadata_section() {
        // Bare WASM header — no sections at all.
        let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let meta = extract_contract_metadata(&wasm).expect("parsing should succeed");
        assert!(meta.is_empty());
    }

    #[test]
    fn extract_metadata_ignores_unrelated_custom_sections() {
        // A custom section with a different name should not affect the result.
        let wasm = make_custom_section_wasm("some_other_section", b"irrelevant data");
        let meta = extract_contract_metadata(&wasm).expect("parsing should succeed");
        assert!(meta.is_empty());
    }

    #[test]
    fn extract_metadata_ignores_non_utf8_payload() {
        // Non-UTF-8 bytes in a contractmeta section must not cause an error.
        let bad_bytes: &[u8] = &[0xFF, 0xFE, 0x00, 0x01];
        let wasm = make_custom_section_wasm("contractmeta", bad_bytes);
        let meta = extract_contract_metadata(&wasm).expect("should not error");
        assert!(meta.is_empty());
    }

    // ── ContractMetadata::is_empty ────────────────────────────────────────────

    #[test]
    fn contract_metadata_is_empty_when_default() {
        assert!(ContractMetadata::default().is_empty());
    }

    #[test]
    fn contract_metadata_not_empty_when_any_field_set() {
        let meta = ContractMetadata {
            contract_version: Some("1.0.0".into()),
            ..Default::default()
        };
        assert!(!meta.is_empty());
    }
}
