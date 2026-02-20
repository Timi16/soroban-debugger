use crate::Result;
use soroban_env_host::{xdr::ContractEventBody, Host};

/// Represents a captured contract event
#[derive(Debug, Clone)]
pub struct ContractEvent {
    pub contract_id: Option<String>,
    pub topics: Vec<String>,
    pub data: String,
}

pub struct EventInspector;

impl EventInspector {
    /// Extract events from the host and convert them to a friendly format
    pub fn get_events(host: &Host) -> Result<Vec<ContractEvent>> {
        let events = host.get_events()?.0;
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

    /// Filter events by a topic string
    pub fn filter_events(events: &[ContractEvent], topic_filter: &str) -> Vec<ContractEvent> {
        events
            .iter()
            .filter(|e| e.topics.iter().any(|t| t.contains(topic_filter)))
            .cloned()
            .collect()
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
