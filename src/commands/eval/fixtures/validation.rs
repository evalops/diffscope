use anyhow::Result;
use regex::Regex;

use super::super::EvalFixture;

pub(super) fn validate_eval_fixture(fixture: &EvalFixture) -> Result<()> {
    for pattern in fixture
        .expect
        .must_find
        .iter()
        .chain(fixture.expect.must_not_find.iter())
    {
        if let Some(pattern_text) = pattern.matches_regex.as_deref().map(str::trim) {
            if !pattern_text.is_empty() {
                Regex::new(pattern_text).map_err(|error| {
                    anyhow::anyhow!(
                        "Invalid regex '{}' in fixture '{}': {}",
                        pattern_text,
                        fixture.name.as_deref().unwrap_or("<unnamed>"),
                        error
                    )
                })?;
            }
        }
    }
    Ok(())
}
