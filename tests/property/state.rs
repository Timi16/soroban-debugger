use proptest::prelude::*;
use soroban_debugger::simulator::state::NetworkSnapshot;

proptest! {
    #[test]
    fn test_snapshot_roundtrip(
        seq in 1u32..1000,
        ts in 0u64..1_000_000,
        pass in "network-passphrase"
    ) {
        let snapshot = NetworkSnapshot::new(seq, pass, ts);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        let deserialized: NetworkSnapshot = serde_json::from_str(&serialized).unwrap();

        prop_assert_eq!(snapshot.ledger.sequence, deserialized.ledger.sequence);
        prop_assert_eq!(snapshot.ledger.timestamp, deserialized.ledger.timestamp);
        prop_assert_eq!(snapshot.ledger.network_passphrase, deserialized.ledger.network_passphrase);
        prop_assert!(snapshot.accounts.is_empty());
        prop_assert!(deserialized.accounts.is_empty());
    }
}
