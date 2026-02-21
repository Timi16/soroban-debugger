pub mod args;
pub mod commands;

pub use args::{
    Cli, Commands, CompareArgs, CompletionsArgs, InspectArgs, InteractiveArgs, ListFunctionsArgs,
    OptimizeArgs, RunArgs, UpgradeCheckArgs, Verbosity,
};
