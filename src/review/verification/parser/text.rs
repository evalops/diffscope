use once_cell::sync::Lazy;
use regex::{Captures, Regex};

use crate::core::Comment;

use super::super::VerificationResult;

pub(super) fn parse_verification_text(
    content: &str,
    comments: &[Comment],
) -> Vec<VerificationResult> {
    content
        .lines()
        .filter_map(|line| parse_finding_line(line, comments))
        .collect()
}

fn parse_finding_line(line: &str, comments: &[Comment]) -> Option<VerificationResult> {
    let captures = finding_pattern().captures(line)?;
    let index = capture_usize(&captures, 1).unwrap_or(0);
    if index == 0 || index > comments.len() {
        return None;
    }

    let accurate = capture_bool(&captures, 3)?;
    let score = capture_u8(&captures, 2)?.min(10);
    let line_correct = capture_bool(&captures, 4).unwrap_or(accurate);
    let suggestion_sound = capture_bool(&captures, 5).unwrap_or(true);
    let reason = captures.get(6)?.as_str().trim().to_string();

    Some(VerificationResult {
        comment_id: comments[index - 1].id.clone(),
        accurate,
        line_correct,
        suggestion_sound,
        score,
        reason,
    })
}

fn finding_pattern() -> &'static Regex {
    static FINDING_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)FINDING\s+(\d+)\s*:\s*score\s*=\s*(\d+)\s+accurate\s*=\s*(true|false)(?:\s+line_correct\s*=\s*(true|false))?(?:\s+suggestion_sound\s*=\s*(true|false))?\s+reason\s*=\s*(.+)")
            .unwrap()
    });
    &FINDING_PATTERN
}

fn capture_usize(captures: &Captures<'_>, group: usize) -> Option<usize> {
    captures.get(group)?.as_str().parse().ok()
}

fn capture_u8(captures: &Captures<'_>, group: usize) -> Option<u8> {
    captures.get(group)?.as_str().parse().ok()
}

fn capture_bool(captures: &Captures<'_>, group: usize) -> Option<bool> {
    captures
        .get(group)
        .map(|value| value.as_str().eq_ignore_ascii_case("true"))
}
