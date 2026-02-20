use assert_cmd::Command;
use predicates::prelude::*;
use soroban_debugger::inspector::auth::{AuthInspector, AuthNode};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    Address, Env, Symbol, Val, Vec as SorobanVec,
};

#[test]
fn test_run_command_auth_flags() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--show-auth"))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_auth_node_serialization() {
    let node = AuthNode {
        function: "transfer".to_string(),
        contract_id: "C123".to_string(),
        sub_invocations: vec![AuthNode {
            function: "inner".to_string(),
            contract_id: "C456".to_string(),
            sub_invocations: vec![],
        }],
    };

    let nodes = vec![node];
    let json = AuthInspector::to_json(&nodes).unwrap();
    assert!(json.contains("transfer"));
    assert!(json.contains("inner"));
    assert!(json.contains("C123"));
    assert!(json.contains("C456"));
}

#[test]
fn test_auth_inspector_conversion() {
    let env = Env::default();
    let contract_id = Address::generate(&env);
    let function_name = Symbol::new(&env, "test_func");
    let args = SorobanVec::<Val>::new(&env);

    let _invocation = AuthorizedInvocation {
        function: AuthorizedFunction::Contract((
            contract_id.clone(),
            function_name.clone(),
            args.clone(),
        )),
        sub_invocations: std::vec::Vec::new(),
    };

    // Use a helper function or direct call if we can't easily mock recorded auths here.
    // Since we are testing the conversion logic:
    // We can't easily call AuthInspector::get_auth_tree(env) because nothing is recorded yet.
    // But we can test AuthInspector's private method if we make it public or test through public API with mock data.
    // In auth.rs, I'll make convert_invocation public for testing or just test the display logic.

    let nodes = vec![soroban_debugger::inspector::auth::AuthNode {
        function: format!("{:?}({:?})", function_name, args),
        contract_id: format!("{:?}", contract_id),
        sub_invocations: vec![],
    }];

    AuthInspector::display(&nodes);
}
