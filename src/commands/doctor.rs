#[path = "doctor/command.rs"]
mod command;
#[path = "doctor/endpoint.rs"]
mod endpoint;
#[path = "doctor/system.rs"]
mod system;

pub use command::doctor_command;
