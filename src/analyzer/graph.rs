use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    nodes: BTreeSet<String>,
    edges: BTreeSet<DependencyEdge>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: impl Into<String>) {
        self.nodes.insert(node.into());
    }

    pub fn add_edge(&mut self, from: impl Into<String>, to: impl Into<String>) {
        let from = from.into();
        let to = to.into();
        self.nodes.insert(from.clone());
        self.nodes.insert(to.clone());
        self.edges.insert(DependencyEdge { from, to });
    }

    pub fn edges(&self) -> Vec<DependencyEdge> {
        self.edges.iter().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    fn escape_dot_label(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn escape_mermaid_label(value: &str) -> String {
        value
            .replace('"', "&quot;")
            .replace('[', "(")
            .replace(']', ")")
            .replace('{', "(")
            .replace('}', ")")
    }

    pub fn to_dot(&self) -> String {
        let mut out = String::from("digraph contract_dependencies {\n");
        out.push_str("  rankdir=LR;\n");

        for node in &self.nodes {
            out.push_str(&format!(
                "  \"{}\";\n",
                Self::escape_dot_label(node.as_str())
            ));
        }

        for edge in &self.edges {
            out.push_str(&format!(
                "  \"{}\" -> \"{}\";\n",
                Self::escape_dot_label(edge.from.as_str()),
                Self::escape_dot_label(edge.to.as_str())
            ));
        }

        out.push('}');
        out
    }

    pub fn to_mermaid(&self) -> String {
        let mut out = String::from("graph LR\n");

        for edge in &self.edges {
            out.push_str(&format!(
                "  \"{}\" --> \"{}\"\n",
                Self::escape_mermaid_label(edge.from.as_str()),
                Self::escape_mermaid_label(edge.to.as_str())
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_export_contains_nodes_and_edges() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("contract_a", "token_contract");

        let dot = graph.to_dot();
        assert!(dot.starts_with("digraph contract_dependencies"));
        assert!(dot.contains("\"contract_a\" -> \"token_contract\";"));
    }

    #[test]
    fn mermaid_export_contains_edges() {
        let mut graph = DependencyGraph::new();
        graph.add_edge("contract_a", "oracle_contract");

        let mermaid = graph.to_mermaid();
        assert!(mermaid.starts_with("graph LR"));
        assert!(mermaid.contains("\"contract_a\" --> \"oracle_contract\""));
    }
}
