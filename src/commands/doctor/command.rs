#[path = "command/display.rs"]
mod display;
#[path = "command/probe.rs"]
mod probe;
#[path = "command/recommend.rs"]
mod recommend;
#[path = "command/run.rs"]
mod run;

pub use run::doctor_command;
