#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_debugger::utils::arguments::ArgumentParser;
use soroban_sdk::Env;

fuzz_target!(|data: &[u8]| {
    if let Ok(json_str) = std::str::from_utf8(data) {
        let env = Env::default();
        let parser = ArgumentParser::new(env);
        let _ = parser.parse_args_string(json_str);
    }
});
