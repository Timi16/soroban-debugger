use super::DebuggerEngine;

fn create_test_engine() -> DebuggerEngine {
    let wasm_bytes = include_bytes!("../../tests/fixtures/wasm/echo.wasm").to_vec();
    let executor = crate::runtime::executor::ContractExecutor::new(wasm_bytes).unwrap();
    DebuggerEngine::new(executor, vec![])
}

fn create_counter_engine(initial_breakpoints: Vec<String>) -> DebuggerEngine {
    let wasm_bytes = include_bytes!("../../tests/fixtures/wasm/counter.wasm").to_vec();
    let executor = crate::runtime::executor::ContractExecutor::new(wasm_bytes).unwrap();
    DebuggerEngine::new(executor, initial_breakpoints)
}

#[test]
fn engine_starts_unpaused() {
    let engine = create_test_engine();
    assert!(!engine.is_paused());
}

#[test]
fn no_source_location_without_instruction_state() {
    let engine = create_test_engine();
    assert!(engine.current_source_location().is_none());
}

#[test]
fn execute_increments_breakpoint_hit_count_once_per_hit() {
    let mut engine = create_counter_engine(vec!["get".to_string()]);

    let _ = engine.execute("get", None);
    assert_eq!(
        engine
            .breakpoints()
            .get_breakpoint("get")
            .unwrap()
            .hit_count,
        1
    );

    let _ = engine.execute("get", None);
    assert_eq!(
        engine
            .breakpoints()
            .get_breakpoint("get")
            .unwrap()
            .hit_count,
        2
    );
}
