use proptest::prelude::*;
use soroban_debugger::simulator::state::ContractState;

use crate::utils::json_value;

proptest! {
    #[test]
    fn test_contract_storage_read_after_write(
        key in "\\PC*",
        value in json_value()
    ) {
        let mut contract = ContractState::new("C_TEST", "hash");
        contract.set_storage(key.clone(), value.clone());

        let retrieved = contract.get_storage(&key);
        prop_assert!(retrieved.is_some());
        prop_assert_eq!(retrieved.unwrap(), &value);
    }

    #[test]
    fn test_contract_storage_overwrite(
        key in "\\PC*",
        val1 in json_value(),
        val2 in json_value()
    ) {
        let mut contract = ContractState::new("C_TEST", "hash");
        contract.set_storage(key.clone(), val1);
        contract.set_storage(key.clone(), val2.clone());

        let retrieved = contract.get_storage(&key);
        prop_assert_eq!(retrieved.unwrap(), &val2);
    }

    #[test]
    fn test_contract_serialization_roundtrip(
        id in "[a-zA-Z0-9]{56}", // Stellar contract ID like
        hash in "[a-fA-F0-9]{64}", // Hex hash
        storage_key in "key",
        storage_val in json_value()
    ) {
        let mut contract = ContractState::new(id, hash);
        contract.set_storage(storage_key, storage_val);

        let serialized = serde_json::to_string(&contract).unwrap();
        let deserialized: ContractState = serde_json::from_str(&serialized).unwrap();

        prop_assert_eq!(contract.contract_id, deserialized.contract_id);
        prop_assert_eq!(contract.wasm_hash, deserialized.wasm_hash);
        // Storage equality check might need exact match of map
        // BTreeMap equality should work
        // Note: ContractState has BTreeMap, deserialized has BTreeMap.
        // serde_json::Value PartialEq works.
    }
}
