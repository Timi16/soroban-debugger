#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_debugger::inspector::storage::{StorageFilter, FilterPattern};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = FilterPattern::parse(s);
        let _ = StorageFilter::new(&[s.to_string()]);
    }
});
