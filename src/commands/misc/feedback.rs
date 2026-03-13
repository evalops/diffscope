#[path = "feedback/apply.rs"]
mod apply;
#[path = "feedback/command.rs"]
mod command;
#[path = "feedback/conventions.rs"]
mod conventions;

pub use command::feedback_command;
