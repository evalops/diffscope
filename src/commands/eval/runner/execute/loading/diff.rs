use anyhow::Result;
use std::path::Path;

use super::super::super::super::EvalFixture;

pub(super) fn load_diff_content(
    fixture_name: &str,
    fixture_dir: &Path,
    fixture: &EvalFixture,
) -> Result<String> {
    match (fixture.diff.clone(), fixture.diff_file.clone()) {
        (Some(diff), _) => Ok(diff),
        (None, Some(diff_file)) => {
            let path = if diff_file.is_absolute() {
                diff_file
            } else {
                fixture_dir.join(diff_file)
            };
            Ok(std::fs::read_to_string(path)?)
        }
        (None, None) => anyhow::bail!(
            "Fixture '{}' must define either diff or diff_file",
            fixture_name
        ),
    }
}
