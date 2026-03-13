use crate::core;

pub(super) fn build_github_comment_body(comment: &core::Comment) -> String {
    let mut body = format!(
        "**{:?} ({:?})**\n\n{}",
        comment.severity, comment.category, comment.content
    );
    if let Some(rule_id) = &comment.rule_id {
        body.push_str(&format!("\n\n**Rule:** `{}`", rule_id));
    }
    if let Some(suggestion) = &comment.suggestion {
        body.push_str("\n\n**Suggested fix:** ");
        body.push_str(suggestion);
    }
    body.push_str(&format!(
        "\n\n_Confidence: {:.0}%_",
        comment.confidence * 100.0
    ));
    body
}
