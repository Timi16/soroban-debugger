#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_debugger::utils::wasm;

fuzz_target!(|data: &[u8]| {
    let _ = wasm::parse_functions(data);
    let _ = wasm::get_module_info(data);
    let _ = wasm::extract_contract_metadata(data);
});
