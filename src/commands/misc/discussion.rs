#[path = "discussion/command.rs"]
mod command;
#[path = "discussion/prompt.rs"]
mod prompt;
#[path = "discussion/selection.rs"]
mod selection;
#[path = "discussion/thread.rs"]
mod thread;
#[path = "discussion/types.rs"]
mod types;

pub use command::discuss_command;
