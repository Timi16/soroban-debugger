use proptest::prelude::*;
use soroban_debugger::inspector::budget::BudgetInfo;

proptest! {
    #[test]
    fn test_cpu_percentage_calculation(
        cpu_consumed in 0u64..10_000_000,
        cpu_limit in 0u64..10_000_000
    ) {
        let info = BudgetInfo {
            cpu_instructions: cpu_consumed,
            cpu_limit,
            memory_bytes: 0,
            memory_limit: 100,
        };

        let cpu_pct = info.cpu_percentage();

        if cpu_limit == 0 {
            prop_assert_eq!(cpu_pct, 0.0);
        } else {
            let expected = (cpu_consumed as f64 / cpu_limit as f64) * 100.0;
             prop_assert!((cpu_pct - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn test_memory_percentage_calculation(
        mem_consumed in 0u64..10_000_000,
        mem_limit in 0u64..10_000_000
    ) {
        let info = BudgetInfo {
            cpu_instructions: 0,
            cpu_limit: 100,
            memory_bytes: mem_consumed,
            memory_limit: mem_limit,
        };

        let mem_pct = info.memory_percentage();

        if mem_limit == 0 {
             prop_assert_eq!(mem_pct, 0.0);
        } else {
            let expected = (mem_consumed as f64 / mem_limit as f64) * 100.0;
            prop_assert!((mem_pct - expected).abs() < 1e-6);
        }
    }
}
