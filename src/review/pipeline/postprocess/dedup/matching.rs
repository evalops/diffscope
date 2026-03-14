use crate::core;

pub(super) fn find_dominated_comment_index(
    deduped: &[core::Comment],
    comment: &core::Comment,
) -> Option<usize> {
    deduped.iter().position(|existing| {
        if existing.file_path != comment.file_path || existing.line_number != comment.line_number {
            return false;
        }

        existing.rule_id.is_some() && existing.rule_id == comment.rule_id
            || dedup_similarity(existing, comment) > 0.5
    })
}

fn dedup_similarity(a: &core::Comment, b: &core::Comment) -> f32 {
    let direct = core::multi_pass::content_similarity(&a.content, &b.content);
    if direct > 0.6 {
        return direct;
    }

    let canonical_a = canonicalize_comment_text(a);
    let canonical_b = canonicalize_comment_text(b);
    let canonical = core::multi_pass::content_similarity(&canonical_a, &canonical_b);
    if canonical > 0.5 {
        canonical
    } else {
        direct.max(canonical)
    }
}

fn canonicalize_comment_text(comment: &core::Comment) -> String {
    let mut canonical =
        format!("{} {}", comment.content, comment.tags.join(" ")).to_ascii_lowercase();
    for (source, replacement) in [
        (
            "piping curl output directly to bash",
            "remote script execution",
        ),
        (
            "piping a remote install script to bash",
            "remote script execution",
        ),
        ("piping a remote script to bash", "remote script execution"),
        (
            "piping remote script directly to bash",
            "remote script execution",
        ),
        ("downloads a script and runs it", "remote script execution"),
        ("downloads and executes a script", "remote script execution"),
        ("downloads and executes script", "remote script execution"),
        (
            "execute unverified code",
            "remote script execution without verification",
        ),
        (
            "without checksum or signature verification",
            "without verification",
        ),
        ("without checksum verification", "without verification"),
        ("without signature verification", "without verification"),
        ("without integrity verification", "without verification"),
        ("unverified code", "without verification"),
        ("promise object is truthy", "missing await"),
        ("promise is always truthy", "missing await"),
        ("authorization bypass", "authorization"),
        ("supply-chain", "supply chain"),
        ("supply chain risk", "supply chain"),
    ] {
        canonical = canonical.replace(source, replacement);
    }
    canonical = canonical
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>();
    canonical
        .split_whitespace()
        .filter_map(|token| {
            let normalized = match token {
                "executes" | "executed" | "executing" => "execute",
                "downloads" | "downloaded" | "downloading" => "download",
                "verification" | "verifies" | "verified" | "verifying" => "verify",
                "permissions" => "permission",
                "risks" => "risk",
                other => other,
            };
            if matches!(
                normalized,
                "a" | "an"
                    | "and"
                    | "as"
                    | "at"
                    | "create"
                    | "creating"
                    | "during"
                    | "for"
                    | "it"
                    | "of"
                    | "or"
                    | "that"
                    | "the"
                    | "this"
                    | "to"
            ) {
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
