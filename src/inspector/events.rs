use crate::{DebuggerError, Result};
use serde::{Deserialize, Serialize};
use soroban_env_host::{xdr::ContractEventBody, Host};

/// Represents a captured contract event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractEvent {
    /// Contract id that emitted the event (if present)
    pub contract_id: Option<String>,

    /// Event topics (ordered)
    pub topics: Vec<String>,

    /// Event data/payload (stringified)
    pub data: String,
}

pub struct EventInspector;

impl EventInspector {
    /// Extract events from the host and convert them to a friendly format
    pub fn get_events(host: &Host) -> Result<Vec<ContractEvent>> {
        let events = host
            .get_events()
            .map_err(|e| DebuggerError::ExecutionError(format!("Failed to get events: {}", e)))?
            .0;
        let mut contract_events = Vec::new();

        for host_event in events.iter() {
            let event = &host_event.event;

            // Extract topics and data from event body
            let (topics, data) = match &event.body {
                ContractEventBody::V0(v0) => {
                    let mut topics = Vec::new();
                    for topic in v0.topics.iter() {
                        topics.push(format!("{:?}", topic));
                    }
                    let data = format!("{:?}", v0.data);
                    (topics, data)
                }
            };

            // Parse contract ID
            // contract_id is Option<Hash>
            let contract_id = event.contract_id.as_ref().map(|h| format!("{:?}", h));

            contract_events.push(ContractEvent {
                contract_id,
                topics,
                data,
            });
        }

        Ok(contract_events)
    }

    /// Filter events by topic substring. If `topic_filter` is empty,
    /// returns a clone of input slice.
    pub fn filter_events(events: &[ContractEvent], topic_filter: &str) -> Vec<ContractEvent> {
        if topic_filter.is_empty() {
            return events.to_vec();
        }
        let topic_filter = topic_filter.to_lowercase();
        events
            .iter()
            .filter(|e| {
                // match if any topic contains the filter substring (case-insensitive)
                e.topics
                    .iter()
                    .any(|t| t.to_lowercase().contains(&topic_filter))
                    // or event data contains the filter (useful)
                    || e.data.to_lowercase().contains(&topic_filter)
            })
            .cloned()
            .collect()
    }

    /// Pretty-print events to stdout (via provided closure that will typically call logging/Formatter).
    /// Here we return a Vec<String> of formatted lines to let the caller decide how to print/log them.
    pub fn format_events(events: &[ContractEvent]) -> Vec<String> {
        let mut out = Vec::new();
        for (i, ev) in events.iter().enumerate() {
            out.push(format!("Event #{}:", i));
            out.push(format!(
                "  Contract: {}",
                ev.contract_id.as_deref().unwrap_or("<none>")
            ));
            out.push(format!("  Topics: {:?}", ev.topics));
            out.push(format!("  Data: {}", ev.data));
        }
        out
    }

    /// Convert events into a serde_json::Value array for inclusion in JSON outputs.
    pub fn to_json_value(events: &[ContractEvent]) -> serde_json::Value {
        let arr: Vec<serde_json::Value> = events
            .iter()
            .map(|e| {
                serde_json::json!({
                    "contract_id": e.contract_id,
                    "topics": e.topics,
                    "data": e.data,
                })
            })
            .collect();
        serde_json::Value::Array(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_events() {
        let events = vec![
            ContractEvent {
                contract_id: None,
                topics: vec!["topic1".to_string(), "common".to_string()],
                data: "data1".to_string(),
            },
            ContractEvent {
                contract_id: None,
                topics: vec!["topic2".to_string(), "common".to_string()],
                data: "data2".to_string(),
            },
            ContractEvent {
                contract_id: None,
                topics: vec!["topic3".to_string()],
                data: "data3".to_string(),
            },
        ];

        let filtered = EventInspector::filter_events(&events, "topic1");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].data, "data1");

        let filtered = EventInspector::filter_events(&events, "common");
        assert_eq!(filtered.len(), 2);

        let filtered = EventInspector::filter_events(&events, "nonexistent");
        assert_eq!(filtered.len(), 0);
    }
}
