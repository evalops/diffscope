pub(super) fn format_guidance_sections(sections: Vec<String>) -> Option<String> {
    if sections.is_empty() {
        None
    } else {
        Some(format!(
            "Additional review guidance:\n{}",
            sections.join("\n\n")
        ))
    }
}
