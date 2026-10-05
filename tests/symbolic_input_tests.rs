// tests/symbolic_input_tests.rs
// Test the symbolic input generation logic and CLI argument validation.

#[cfg(test)]
mod tests {
    use clap::Parser;
    use soroban_debugger::cli::args::{Cli, Commands};

    /// Test that --seed and --replay are mutually exclusive.
    #[test]
    fn symbolic_seed_and_replay_are_mutually_exclusive() {
        let result = Cli::try_parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "test",
            "--seed",
            "12345",
            "--replay",
            "67890",
        ]);

        // Clap should reject this due to conflicts_with
        assert!(
            result.is_err(),
            "Expected error when both --seed and --replay are provided"
        );
        let err = result.err().expect("expected a parse error");
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("seed") && err_msg.contains("replay"),
            "Error message should mention both seed and replay flags: {}",
            err_msg
        );
    }

    /// Test that --seed alone is accepted.
    #[test]
    fn symbolic_seed_alone_is_valid() {
        let cli = Cli::try_parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "test",
            "--seed",
            "12345",
        ])
        .expect("Failed to parse CLI with --seed");

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.seed, Some(12345));
        assert_eq!(args.replay, None);
    }

    /// Test that --replay alone is accepted.
    #[test]
    fn symbolic_replay_alone_is_valid() {
        let cli = Cli::try_parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "test",
            "--replay",
            "67890",
        ])
        .expect("Failed to parse CLI with --replay");

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.replay, Some(67890));
        assert_eq!(args.seed, None);
    }

    /// Test that neither --seed nor --replay is required.
    #[test]
    fn symbolic_neither_seed_nor_replay_is_valid() {
        let cli = Cli::try_parse_from([
            "soroban-debug",
            "symbolic",
            "--contract",
            "contract.wasm",
            "--function",
            "test",
        ])
        .expect("Failed to parse CLI without --seed or --replay");

        let Commands::Symbolic(args) = cli.command.expect("symbolic command expected") else {
            panic!("symbolic command expected");
        };

        assert_eq!(args.seed, None);
        assert_eq!(args.replay, None);
    }
}
