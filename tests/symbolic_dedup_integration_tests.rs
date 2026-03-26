use soroban_debugger::analyzer::symbolic::{SymbolicAnalyzer, SymbolicConfig};

fn fixture_wasm(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join(format!("{name}.wasm"))
}

#[test]
fn symbolic_preserves_distinct_inputs_with_same_return_value() {
    let wasm = fixture_wasm("same_return");
    if !wasm.exists() {
        eprintln!(
            "Skipping test: fixture not found at {}. Run tests/fixtures/build.sh to build fixtures.",
            wasm.display()
        );
        return;
    }

    let bytes = std::fs::read(&wasm).unwrap();
    let analyzer = SymbolicAnalyzer::new();
    let report = analyzer.analyze(&bytes, "same").expect("analysis failed");

    assert!(
        report.paths.len() >= 2,
        "expected at least two paths, got {}",
        report.paths.len()
    );

    // Find any return value that appears for at least 2 distinct inputs.
    let mut found = false;
    for i in 0..report.paths.len() {
        for j in (i + 1)..report.paths.len() {
            let a = &report.paths[i];
            let b = &report.paths[j];
            if a.inputs != b.inputs && a.return_value == b.return_value {
                found = true;
                break;
            }
        }
        if found {
            break;
        }
    }

    assert!(
        found,
        "expected two distinct inputs to share the same return value"
    );
}

// ── Seed / replay tests ──────────────────────────────────────────────────────

/// Running the same seed twice must produce an identical exploration order.
#[test]
fn same_seed_produces_identical_exploration_order() {
    let wasm = fixture_wasm("counter");
    if !wasm.exists() {
        eprintln!("Skipping test: counter fixture not found.");
        return;
    }

    let bytes = std::fs::read(&wasm).unwrap();
    let analyzer = SymbolicAnalyzer::new();

    let config = SymbolicConfig {
        max_paths: 10,
        max_input_combinations: 20,
        timeout_secs: 30,
        seed: Some(12345),
    };

    let report_a = analyzer
        .analyze_with_config(&bytes, "increment", &config)
        .expect("analysis a failed");
    let report_b = analyzer
        .analyze_with_config(&bytes, "increment", &config)
        .expect("analysis b failed");

    let order_a: Vec<_> = report_a.paths.iter().map(|p| p.inputs.clone()).collect();
    let order_b: Vec<_> = report_b.paths.iter().map(|p| p.inputs.clone()).collect();

    assert_eq!(
        order_a, order_b,
        "same seed must yield the same exploration order"
    );
    assert_eq!(report_a.metadata.seed, Some(12345));
}

/// Two different seeds must (in practice) produce different exploration orders.
#[test]
fn different_seeds_produce_different_exploration_order() {
    let wasm = fixture_wasm("counter");
    if !wasm.exists() {
        eprintln!("Skipping test: counter fixture not found.");
        return;
    }

    let bytes = std::fs::read(&wasm).unwrap();
    let analyzer = SymbolicAnalyzer::new();

    let config_a = SymbolicConfig {
        max_paths: 6,
        max_input_combinations: 6,
        timeout_secs: 30,
        seed: Some(1),
    };
    let config_b = SymbolicConfig {
        seed: Some(2),
        ..config_a.clone()
    };

    let report_a = analyzer
        .analyze_with_config(&bytes, "increment", &config_a)
        .unwrap();
    let report_b = analyzer
        .analyze_with_config(&bytes, "increment", &config_b)
        .unwrap();

    // Only assert when there are enough paths to expect a reordering.
    if report_a.paths.len() > 1 && report_b.paths.len() > 1 {
        let order_a: Vec<_> = report_a.paths.iter().map(|p| p.inputs.clone()).collect();
        let order_b: Vec<_> = report_b.paths.iter().map(|p| p.inputs.clone()).collect();
        assert_ne!(
            order_a, order_b,
            "different seeds should yield different orders"
        );
    }
}

/// Running without a seed preserves the original deterministic (un-shuffled)
/// order and records `seed: None` in the metadata.
#[test]
fn no_seed_records_none_in_metadata() {
    let wasm = fixture_wasm("counter");
    if !wasm.exists() {
        eprintln!("Skipping test: counter fixture not found.");
        return;
    }

    let bytes = std::fs::read(&wasm).unwrap();
    let analyzer = SymbolicAnalyzer::new();

    let report = analyzer
        .analyze(&bytes, "increment")
        .expect("analysis failed");

    assert_eq!(report.metadata.seed, None);
}
