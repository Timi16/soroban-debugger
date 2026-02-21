#![cfg(any())]
use soroban_debugger::debugger::timeline::{ExecutionSnapshot, TimelineManager};
use soroban_debugger::inspector::budget::BudgetInfo;
use std::collections::HashMap;

#[test]
fn test_timeline_push_and_navigation() {
    let mut tm = TimelineManager::new(10);

    let snap1 = create_snap(1, 10, "func1");
    let snap2 = create_snap(2, 20, "func2");

    tm.push(snap1);
    tm.push(snap2);

    assert_eq!(tm.len(), 2);
    assert_eq!(tm.current_pos(), 1);

    // Step back
    let back = tm.step_back().unwrap();
    assert_eq!(back.step, 1);
    assert_eq!(tm.current_pos(), 0);

    // Step forward
    let forward = tm.step_forward().unwrap();
    assert_eq!(forward.step, 2);
    assert_eq!(tm.current_pos(), 1);
}

#[test]
fn test_timeline_truncate_on_push_after_back() {
    let mut tm = TimelineManager::new(10);

    tm.push(create_snap(1, 10, "f1"));
    tm.push(create_snap(2, 20, "f1"));
    tm.push(create_snap(3, 30, "f1"));

    tm.step_back(); // Now at 2

    // Push new snapshot, should remove 3
    tm.push(create_snap(4, 40, "f2"));

    assert_eq!(tm.len(), 3); // [1, 2, 4]
    assert_eq!(tm.current().unwrap().step, 4);
    assert_eq!(tm.step_back().unwrap().step, 2);
}

#[test]
fn test_timeline_goto() {
    let mut tm = TimelineManager::new(10);

    tm.push(create_snap(1, 10, "f1"));
    tm.push(create_snap(5, 50, "f1"));
    tm.push(create_snap(10, 100, "f1"));

    let snap = tm.goto(5).unwrap();
    assert_eq!(snap.step, 5);
    assert_eq!(tm.current_pos(), 1);

    assert!(tm.goto(99).is_none());
}

fn create_snap(step: usize, ip: usize, func: &str) -> ExecutionSnapshot {
    ExecutionSnapshot {
        step,
        instruction_index: ip,
        function: func.to_string(),
        call_stack: vec![],
        storage: HashMap::new(),
        budget: BudgetInfo {
            cpu_instructions: 0,
            cpu_limit: 100,
            memory_bytes: 0,
            memory_limit: 100,
        },
        events_count: 0,
        timestamp: 0,
    }
}
