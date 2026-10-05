//! Tests for Issue #837: Remote Capability Negotiation

#[cfg(test)]
mod capability_negotiation {
    use soroban_debugger::server::protocol::{
        DebugRequest, DebugResponse, ServerCapabilities, PROTOCOL_MAX_VERSION, PROTOCOL_MIN_VERSION,
    };

    #[test]
    fn test_server_capabilities_current_build() {
        let caps = ServerCapabilities::current();
        assert!(caps.conditional_breakpoints);
        assert!(caps.source_breakpoints);
        assert!(caps.evaluate);
        assert!(caps.tls);
        assert!(caps.token_auth);
        assert!(caps.session_lifecycle);
        assert!(caps.repeat_execution);
        assert!(!caps.symbolic_analysis);
        assert!(caps.snapshot_loading);
        assert!(caps.dynamic_trace_events);
    }

    #[test]
    fn test_server_capabilities_default_is_empty() {
        let caps = ServerCapabilities::default();
        assert!(!caps.conditional_breakpoints);
        assert!(!caps.source_breakpoints);
        assert!(!caps.evaluate);
        assert!(!caps.tls);
        assert!(!caps.token_auth);
        assert!(!caps.session_lifecycle);
        assert!(!caps.repeat_execution);
        assert!(!caps.symbolic_analysis);
        assert!(!caps.snapshot_loading);
        assert!(!caps.dynamic_trace_events);
    }

    #[test]
    fn test_unsupported_by_identifies_missing_features() {
        let client_required = ServerCapabilities {
            evaluate: true,
            snapshot_loading: true,
            conditional_breakpoints: true,
            ..Default::default()
        };

        let server_has = ServerCapabilities {
            evaluate: true,
            snapshot_loading: false,
            conditional_breakpoints: true,
            ..Default::default()
        };

        let missing = client_required.unsupported_by(&server_has);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains(&"snapshot_loading"));
    }

    #[test]
    fn test_unsupported_by_returns_empty_when_all_supported() {
        let client_required = ServerCapabilities {
            evaluate: true,
            conditional_breakpoints: true,
            ..Default::default()
        };

        let server_has = ServerCapabilities::current();
        let missing = client_required.unsupported_by(&server_has);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_handshake_request_with_required_capabilities() {
        let required = ServerCapabilities {
            evaluate: true,
            snapshot_loading: true,
            ..Default::default()
        };

        let request = DebugRequest::Handshake {
            client_name: "test-client".to_string(),
            client_version: "1.0.0".to_string(),
            protocol_min: PROTOCOL_MIN_VERSION,
            protocol_max: PROTOCOL_MAX_VERSION,
            heartbeat_interval_ms: None,
            idle_timeout_ms: None,
            session_label: None,
            reconnect_session_id: None,
            required_capabilities: Some(required.clone()),
        };

        let json = serde_json::to_string(&request).expect("Should serialize");
        assert!(json.contains("required_capabilities"));

        let deserialized: DebugRequest = serde_json::from_str(&json).expect("Should deserialize");
        match deserialized {
            DebugRequest::Handshake {
                required_capabilities: Some(caps),
                ..
            } => {
                assert!(caps.evaluate);
                assert!(caps.snapshot_loading);
            }
            _ => panic!("Expected Handshake with required_capabilities"),
        }
    }

    #[test]
    fn test_handshake_ack_includes_server_capabilities() {
        let server_caps = ServerCapabilities::current();

        let response = DebugResponse::HandshakeAck {
            server_name: "soroban-debug".to_string(),
            server_version: "1.0.0".to_string(),
            protocol_min: PROTOCOL_MIN_VERSION,
            protocol_max: PROTOCOL_MAX_VERSION,
            selected_version: 1,
            session_id: None,
            session_created_at: None,
            session_label: None,
            heartbeat_interval_ms: None,
            idle_timeout_ms: None,
            reconnect_id: None,
            server_capabilities: server_caps.clone(),
        };

        let json = serde_json::to_string(&response).expect("Should serialize");
        assert!(json.contains("server_capabilities"));

        let deserialized: DebugResponse = serde_json::from_str(&json).expect("Should deserialize");
        match deserialized {
            DebugResponse::HandshakeAck {
                server_capabilities: caps,
                ..
            } => {
                assert_eq!(caps.evaluate, server_caps.evaluate);
                assert_eq!(caps.snapshot_loading, server_caps.snapshot_loading);
            }
            _ => panic!("Expected HandshakeAck with server_capabilities"),
        }
    }

    #[test]
    fn test_incompatible_capabilities_response() {
        let server_caps = ServerCapabilities {
            evaluate: true,
            snapshot_loading: false,
            ..Default::default()
        };

        let response = DebugResponse::IncompatibleCapabilities {
            message: "Server does not support required capabilities: snapshot_loading".to_string(),
            missing_capabilities: vec!["snapshot_loading".to_string()],
            server_capabilities: server_caps.clone(),
        };

        let json = serde_json::to_string(&response).expect("Should serialize");
        assert!(json.contains("IncompatibleCapabilities"));
        assert!(json.contains("missing_capabilities"));

        let deserialized: DebugResponse = serde_json::from_str(&json).expect("Should deserialize");
        match deserialized {
            DebugResponse::IncompatibleCapabilities {
                missing_capabilities,
                server_capabilities: caps,
                ..
            } => {
                assert_eq!(missing_capabilities.len(), 1);
                assert_eq!(missing_capabilities[0], "snapshot_loading");
                assert!(!caps.snapshot_loading);
            }
            _ => panic!("Expected IncompatibleCapabilities response"),
        }
    }

    #[test]
    fn test_scenario_client_requires_feature_server_has_it() {
        let client_required = ServerCapabilities {
            evaluate: true,
            snapshot_loading: true,
            ..Default::default()
        };

        let server_has = ServerCapabilities::current();
        let missing = client_required.unsupported_by(&server_has);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_scenario_client_requires_feature_server_lacks_it() {
        let client_required = ServerCapabilities {
            evaluate: true,
            snapshot_loading: true,
            symbolic_analysis: true,
            ..Default::default()
        };

        let server_has = ServerCapabilities::current();
        let missing = client_required.unsupported_by(&server_has);
        assert!(!missing.is_empty());
        assert!(missing.contains(&"symbolic_analysis"));
    }

    #[test]
    fn test_multiple_missing_capabilities_reported() {
        let client_required = ServerCapabilities {
            evaluate: true,
            snapshot_loading: true,
            symbolic_analysis: true,
            dynamic_trace_events: true,
            ..Default::default()
        };

        let server_has = ServerCapabilities {
            evaluate: true,
            snapshot_loading: false,
            symbolic_analysis: false,
            dynamic_trace_events: false,
            ..Default::default()
        };

        let missing = client_required.unsupported_by(&server_has);
        assert_eq!(missing.len(), 3);
        assert!(missing.contains(&"snapshot_loading"));
        assert!(missing.contains(&"symbolic_analysis"));
        assert!(missing.contains(&"dynamic_trace_events"));
    }

    #[test]
    fn test_issue_837_acceptance_criteria() {
        let client_required = ServerCapabilities {
            snapshot_loading: true,
            ..Default::default()
        };

        let server_has = ServerCapabilities {
            snapshot_loading: false,
            ..Default::default()
        };

        let missing = client_required.unsupported_by(&server_has);
        assert!(!missing.is_empty());
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], "snapshot_loading");

        let error_response = DebugResponse::IncompatibleCapabilities {
            message: format!(
                "Server does not support required capabilities: {}. Upgrade the server or disable these features on the client.",
                missing.join(", ")
            ),
            missing_capabilities: missing.iter().map(|s| s.to_string()).collect(),
            server_capabilities: server_has,
        };

        let json = serde_json::to_string(&error_response).expect("Should serialize");
        assert!(json.contains("IncompatibleCapabilities"));
        assert!(json.contains("snapshot_loading"));
    }
}
