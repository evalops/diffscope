use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use crate::core;

pub async fn changelog_command(
    from: Option<String>,
    to: Option<String>,
    release: Option<String>,
    output_path: Option<PathBuf>,
) -> Result<()> {
    info!("Generating changelog/release notes");

    let generator = core::ChangelogGenerator::new(".")?;

    let output = if let Some(version) = release {
        info!("Generating release notes for version {}", version);
        generator.generate_release_notes(&version, from.as_deref())?
    } else {
        let to_ref = to.as_deref().unwrap_or("HEAD");
        info!("Generating changelog from {:?} to {}", from, to_ref);
        generator.generate_changelog(from.as_deref(), to_ref)?
    };

    if let Some(path) = output_path {
        tokio::fs::write(path, output).await?;
        info!("Changelog written to file");
    } else {
        println!("{}", output);
    }

    Ok(())
}
