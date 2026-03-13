#[path = "changelog/generate.rs"]
mod generate;
#[path = "changelog/output.rs"]
mod output;

use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use generate::generate_changelog_output;
use output::emit_changelog_output;

pub async fn changelog_command(
    from: Option<String>,
    to: Option<String>,
    release: Option<String>,
    output_path: Option<PathBuf>,
) -> Result<()> {
    info!("Generating changelog/release notes");
    let output = generate_changelog_output(from.as_deref(), to.as_deref(), release.as_deref())?;
    emit_changelog_output(output_path.as_deref(), &output).await
}
