use std::collections::HashSet;

/// Manages breakpoints during debugging
pub struct BreakpointManager {
    breakpoints: HashSet<String>,
}

impl BreakpointManager {
    /// Create a new breakpoint manager
    pub fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
        }
    }

    /// Add a breakpoint at a function name
    pub fn add(&mut self, function: &str) {
        self.breakpoints.insert(function.to_string());
    }

    /// Remove a breakpoint
    pub fn remove(&mut self, function: &str) -> bool {
        self.breakpoints.remove(function)
    }

    /// Check if execution should break at this function
    pub fn should_break(&self, function: &str) -> bool {
        self.breakpoints.contains(function)
    }

    /// List all breakpoints
    pub fn list(&self) -> Vec<String> {
        self.breakpoints.iter().cloned().collect()
    }

    /// Clear all breakpoints
    pub fn clear(&mut self) {
        self.breakpoints.clear();
    }

    /// Check if there are any breakpoints set
    pub fn is_empty(&self) -> bool {
        self.breakpoints.is_empty()
    }

    /// Get count of breakpoints
    pub fn count(&self) -> usize {
        self.breakpoints.len()
    }

    /// Parse a condition string into a Condition object
    pub fn parse_condition(s: &str) -> crate::Result<Condition> {
        use crate::DebuggerError;

        // storage[key] > value
        if s.starts_with("storage[") {
            let end_bracket = s.find(']').ok_or_else(|| {
                DebuggerError::BreakpointError(
                    "Missing closed bracket ']' in storage condition".to_string(),
                )
            })?;
            let key = s[8..end_bracket].to_string();
            let rem = s[end_bracket + 1..].trim();

            let (op, val_str) = self::split_op_value(rem).map_err(|e| {
                DebuggerError::BreakpointError(format!("Invalid storage condition: {}", e))
            })?;
            return Ok(Condition::Storage {
                key,
                operator: op,
                value: val_str,
            });
        }

        // name > value
        let (op, _) = self::find_operator(s).ok_or_else(|| {
            DebuggerError::BreakpointError(
                "No operator found (use ==, !=, >, <, >=, <=)".to_string(),
            )
        })?;
        let op_pos = s.find(op).unwrap();
        let name = s[..op_pos].trim().to_string();
        let val_str = s[op_pos + op.len()..].trim().to_string();
        let operator = match op {
            "==" => Operator::Eq,
            "!=" => Operator::Ne,
            ">=" => Operator::Ge,
            "<=" => Operator::Le,
            ">" => Operator::Gt,
            "<" => Operator::Lt,
            _ => {
                return Err(
                    DebuggerError::BreakpointError(format!("Unsupported operator: {}", op)).into(),
                )
            }
        };

        Ok(Condition::Argument {
            name,
            operator,
            value: val_str,
        })
    }
}

fn find_operator(s: &str) -> Option<(&'static str, usize)> {
    let ops = [">=", "<=", "==", "!=", ">", "<"];
    for op in ops {
        if let Some(pos) = s.find(op) {
            return Some((op, pos));
        }
    }
    None
}

fn split_op_value(s: &str) -> Result<(Operator, String), String> {
    if s.starts_with("==") {
        Ok((Operator::Eq, s[2..].trim().to_string()))
    } else if s.starts_with("!=") {
        Ok((Operator::Ne, s[2..].trim().to_string()))
    } else if s.starts_with(">=") {
        Ok((Operator::Ge, s[2..].trim().to_string()))
    } else if s.starts_with("<=") {
        Ok((Operator::Le, s[2..].trim().to_string()))
    } else if s.starts_with(">") {
        Ok((Operator::Gt, s[1..].trim().to_string()))
    } else if s.starts_with("<") {
        Ok((Operator::Lt, s[1..].trim().to_string()))
    } else {
        Err(format!("Invalid operator in condition: {}", s))
    }
}

impl Default for BreakpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_breakpoint() {
        let mut manager = BreakpointManager::new();
        manager.add("transfer");
        assert!(manager.should_break("transfer"));
        assert!(!manager.should_break("mint"));
    }

    #[test]
    fn test_remove_breakpoint() {
        let mut manager = BreakpointManager::new();
        manager.add("transfer");
        assert!(manager.remove("transfer"));
        assert!(!manager.should_break("transfer"));
    }

    #[test]
    fn test_list_breakpoints() {
        let mut manager = BreakpointManager::new();
        manager.add("transfer");
        manager.add("mint");
        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"transfer".to_string()));
        assert!(list.contains(&"mint".to_string()));
    }
}
