use criterion::{black_box, criterion_group, criterion_main, Criterion};
use soroban_debugger::debugger::breakpoint::BreakpointManager;
use soroban_debugger::inspector::{BudgetInspector, CallStackInspector, StorageInspector};
use soroban_debugger::inspector::{CallStackInspector, StorageInspector};
use soroban_debugger::utils::arguments::ArgumentParser;
use soroban_sdk::Env;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

fn bench_wasm_loading(c: &mut Criterion) {
    let mut file = NamedTempFile::new().unwrap();
    let dummy_wasm = vec![0u8; 100 * 1024]; // 100KB dummy wasm
    file.write_all(&dummy_wasm).unwrap();
    let path = file.path().to_owned();

    c.bench_function("wasm_loading_100kb", |b| {
        b.iter(|| {
            let bytes = fs::read(black_box(&path)).unwrap();
            black_box(bytes);
        })
    });
}

fn bench_execution_state_management(c: &mut Criterion) {
    // Benchmarks the overhead of managing breakpoints and call stack during execution
    let mut bp_manager = BreakpointManager::new();
    for i in 0..100 {
        bp_manager.add(&format!("func_{}", i));
    }

    c.bench_function("breakpoint_check_100_set", |b| {
        b.iter(|| {
            black_box(bp_manager.should_break("func_50"));
        })
    });

    let mut stack = CallStackInspector::new();
    c.bench_function("call_stack_push_pop", |b| {
        b.iter(|| {
            stack.push("test_func".to_string(), Some("contract_id".to_string()));
            black_box(stack.pop());
        })
    });
}

fn bench_argument_parsing(c: &mut Criterion) {
    let complex_json = r#"[
        {"type": "u32", "value": 42},
        {"type": "symbol", "value": "hello"},
        {"type": "i128", "value": -100},
        {"user": "alice", "balance": 1000, "active": true, "tags": ["admin", "verified"]}
    ]"#;

    c.bench_function("argument_parsing_complex", |b| {
        let env = Env::default();
        let parser = ArgumentParser::new(env);
        b.iter(|| {
            let result = parser.parse_args_string(black_box(complex_json)).unwrap();
            black_box(result);
        })
    });
}

fn bench_storage_snapshot_and_diff(c: &mut Criterion) {
    let mut inspector = StorageInspector::new();
    let mut inspector2 = StorageInspector::new();
    for i in 0..1000 {
        inspector.set(format!("key_{}", i), format!("value_{}", i));
        inspector2.set(
            format!("key_{}", i),
            if i % 2 == 0 {
                format!("mod_{}", i)
            } else {
                format!("value_{}", i)
            },
        );
    }

    c.bench_function("storage_snapshot_1000", |b| {
        b.iter(|| {
            black_box(inspector.get_all());
        })
    });

    c.bench_function("storage_diff_1000", |b| {
        b.iter(|| {
            let s1 = inspector.get_all();
            let s2 = inspector2.get_all();
            let mut diff_count = 0;
            for (k, v1) in s1 {
                if let Some(v2) = s2.get(k) {
                    if v1 != v2 {
                        diff_count += 1;
                    }
                }
            }
            black_box(diff_count);
        })
    });
}

criterion_group!(
    benches,
    bench_wasm_loading,
    bench_execution_state_management,
    bench_argument_parsing,
    bench_storage_snapshot_and_diff
);
criterion_main!(benches);
