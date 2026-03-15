#[path = "mcp/protocol.rs"]
mod protocol;
#[path = "mcp/stdio.rs"]
mod stdio;

pub(crate) use stdio::start_mcp_server;
