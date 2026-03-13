use std::collections::HashMap;

pub(super) struct LanguageMapAudit {
    pub(super) normalized: HashMap<String, String>,
    pub(super) invalid_entries: Vec<String>,
}

pub(super) fn audit_language_map(
    configured_languages: &HashMap<String, String>,
) -> LanguageMapAudit {
    let mut normalized = HashMap::new();
    let mut invalid_entries = Vec::new();

    for (ext, language) in configured_languages {
        let ext = ext.trim().to_ascii_lowercase();
        let language = language.trim().to_string();
        if ext.is_empty() || language.is_empty() {
            invalid_entries.push(format!("{}:{}", ext, language));
            continue;
        }
        normalized.insert(ext, language);
    }

    LanguageMapAudit {
        normalized,
        invalid_entries,
    }
}
