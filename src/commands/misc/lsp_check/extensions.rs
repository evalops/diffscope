use std::collections::HashMap;

pub(super) struct ExtensionAudit {
    pub(super) counts: HashMap<String, usize>,
    pub(super) top_extensions: Vec<String>,
    pub(super) unmapped: Vec<String>,
    pub(super) unused: Vec<String>,
}

pub(super) fn audit_extension_counts(
    extension_counts: HashMap<String, usize>,
    normalized_languages: &HashMap<String, String>,
) -> ExtensionAudit {
    let mut extension_list: Vec<_> = extension_counts.iter().collect();
    extension_list.sort_by(|(a_ext, a_count), (b_ext, b_count)| {
        b_count.cmp(a_count).then_with(|| a_ext.cmp(b_ext))
    });
    let top_extensions = extension_list
        .iter()
        .take(10)
        .map(|(ext, count)| format!("{ext}({count})"))
        .collect();

    let mut unmapped = extension_counts
        .keys()
        .filter(|ext| !normalized_languages.contains_key(*ext))
        .cloned()
        .collect::<Vec<_>>();
    unmapped.sort();

    let mut unused = normalized_languages
        .keys()
        .filter(|ext| !extension_counts.contains_key(*ext))
        .cloned()
        .collect::<Vec<_>>();
    unused.sort();

    ExtensionAudit {
        counts: extension_counts,
        top_extensions,
        unmapped,
        unused,
    }
}
