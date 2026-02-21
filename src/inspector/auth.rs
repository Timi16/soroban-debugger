use crate::Result;
use serde::{Deserialize, Serialize};
use soroban_sdk::{
    testutils::{AuthorizedFunction, AuthorizedInvocation},
    Env,
};

/// Represents a node in the authorization tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthNode {
    pub function: String,
    pub contract_id: String,
    pub sub_invocations: Vec<AuthNode>,
}

pub struct AuthInspector;

impl AuthInspector {
    /// Extract the authorization tree from the environment
    pub fn get_auth_tree(env: &Env) -> Result<Vec<AuthNode>> {
        let recorded_auths = env.auths();
        let mut nodes = Vec::new();

        for (_address, invocation) in recorded_auths {
            nodes.push(Self::convert_invocation(&invocation));
        }

        Ok(nodes)
    }

    fn convert_invocation(inv: &AuthorizedInvocation) -> AuthNode {
        let (function, contract_id) = match &inv.function {
            AuthorizedFunction::Contract(call) => {
                let contract_id = format!("{:?}", call.0);
                let function = format!("{:?}({:?})", call.1, call.2);
                (function, contract_id)
            }
            AuthorizedFunction::CreateContractHostFn(create_fn) => {
                let contract_id = "Host".to_string();
                let function = format!("create_contract({:?})", create_fn);
                (function, contract_id)
            }
            AuthorizedFunction::CreateContractV2HostFn(create_fn) => {
                let contract_id = "Host".to_string();
                let function = format!("create_contract_v2({:?})", create_fn);
                (function, contract_id)
            }
        };

        let sub_invocations = inv
            .sub_invocations
            .iter()
            .map(Self::convert_invocation)
            .collect();

        AuthNode {
            function,
            contract_id,
            sub_invocations,
        }
    }

    /// Display the authorization tree in a human-readable format
    pub fn display(nodes: &[AuthNode]) {
        if nodes.is_empty() {
            println!("No authorizations recorded.");
            return;
        }

        println!("Authorization Tree:");
        for node in nodes {
            Self::print_node(node, 0, true);
        }
    }

    fn print_node(node: &AuthNode, indent: usize, is_last: bool) {
        let prefix = if indent == 0 {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };

        let indent_str = "    ".repeat(indent.saturating_sub(1));
        let full_prefix = if indent > 0 {
            format!("{}{}", indent_str, prefix)
        } else {
            "".to_string()
        };

        println!(
            "{}{} [Contract: {}]",
            full_prefix, node.function, node.contract_id
        );

        for (i, sub) in node.sub_invocations.iter().enumerate() {
            Self::print_node(sub, indent + 1, i == node.sub_invocations.len() - 1);
        }
    }

    /// Return the authorization tree as a JSON string
    pub fn to_json(nodes: &[AuthNode]) -> Result<String> {
        Ok(serde_json::to_string_pretty(nodes)?)
    }
}
