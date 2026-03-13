use anyhow::Result;
use std::path::Path;
use tracing::info;

pub(super) async fn emit_changelog_output(output_path: Option<&Path>, output: &str) -> Result<()> {
    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
        info!("Changelog written to file");
    } else {
        println!("{}", output);
    }

    Ok(())
}
