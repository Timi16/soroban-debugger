use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandOutput<T>
where
    T: Serialize,
{
    pub status: String,
    pub result: Option<T>,
    pub budget: Option<BudgetInfo>,
    pub errors: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct BudgetInfo {
    pub cpu_instructions: u64,
    pub memory_bytes: u64,
}
