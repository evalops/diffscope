use anyhow::Result;
use tracing::info;

use crate::core;

pub(super) fn generate_changelog_output(
    from: Option<&str>,
    to: Option<&str>,
    release: Option<&str>,
) -> Result<String> {
    let generator = core::ChangelogGenerator::new(".")?;

    if let Some(version) = release {
        info!("Generating release notes for version {}", version);
        generator.generate_release_notes(version, from)
    } else {
        let to_ref = to.unwrap_or("HEAD");
        info!("Generating changelog from {:?} to {}", from, to_ref);
        generator.generate_changelog(from, to_ref)
    }
}
