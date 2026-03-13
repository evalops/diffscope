use anyhow::Result;

use crate::core;
use crate::plugins;

pub(in super::super) fn synthesize_analyzer_comments(
    findings: Vec<plugins::AnalyzerFinding>,
) -> Result<Vec<core::Comment>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }

    let raw_comments = findings
        .into_iter()
        .map(|finding| finding.into_raw_comment())
        .collect::<Vec<_>>();
    core::CommentSynthesizer::synthesize(raw_comments)
}

pub(in super::super) fn is_analyzer_comment(comment: &core::Comment) -> bool {
    comment.tags.iter().any(|tag| tag.starts_with("source:"))
}
