use anyhow::Result;

use crate::config;
use crate::core;
use crate::parsing::parse_llm_response;
use crate::review::apply_rule_overrides;

use super::super::overrides::{apply_path_severity_overrides, apply_specialized_pass_tags};

pub(super) fn extract_processed_comments(
    response_content: &str,
    diff: &core::UnifiedDiff,
    active_rules: &[core::ReviewRule],
    path_config: Option<&config::PathConfig>,
    pass_kind: Option<core::SpecializedPassKind>,
) -> Result<Option<Vec<core::Comment>>> {
    let Ok(raw_comments) = parse_llm_response(response_content, &diff.file_path) else {
        return Ok(None);
    };
    if raw_comments.is_empty() {
        return Ok(None);
    }

    let mut comments = core::CommentSynthesizer::synthesize(raw_comments)?;
    apply_specialized_pass_tags(&mut comments, pass_kind);
    apply_path_severity_overrides(&mut comments, path_config);

    let comments = apply_rule_overrides(comments, active_rules);
    if comments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(comments))
    }
}
