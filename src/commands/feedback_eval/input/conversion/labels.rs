pub(super) fn normalize_feedback_label(label: &str) -> Option<bool> {
    match label.trim().to_ascii_lowercase().as_str() {
        "accept" | "accepted" => Some(true),
        "reject" | "rejected" => Some(false),
        _ => None,
    }
}
