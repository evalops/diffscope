#[path = "run/command.rs"]
mod command;
#[path = "run/endpoint.rs"]
mod endpoint;
#[path = "run/recommendation.rs"]
mod recommendation;
#[cfg(test)]
#[path = "run/tests.rs"]
mod tests;

pub use command::doctor_command;
