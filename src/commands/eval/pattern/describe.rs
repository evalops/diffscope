use super::super::EvalPattern;

impl EvalPattern {
    pub(in super::super) fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(file) = &self.file {
            let file = file.trim();
            if !file.is_empty() {
                parts.push(format!("file={}", file));
            }
        }
        if let Some(line) = self.line {
            parts.push(format!("line={}", line));
        }
        if let Some(contains) = &self.contains {
            let contains = contains.trim();
            if !contains.is_empty() {
                parts.push(format!("contains='{}'", contains));
            }
        }
        let contains_any: Vec<&str> = self
            .contains_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !contains_any.is_empty() {
            parts.push(format!("contains_any={}", contains_any.join("|")));
        }
        if let Some(pattern) = self.matches_regex.as_deref().map(str::trim) {
            if !pattern.is_empty() {
                parts.push(format!("matches_regex='{}'", pattern));
            }
        }
        if let Some(severity) = &self.severity {
            let severity = severity.trim();
            if !severity.is_empty() {
                parts.push(format!("severity={}", severity));
            }
        }
        if let Some(category) = &self.category {
            let category = category.trim();
            if !category.is_empty() {
                parts.push(format!("category={}", category));
            }
        }
        let tags_any: Vec<&str> = self
            .tags_any
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect();
        if !tags_any.is_empty() {
            parts.push(format!("tags_any={}", tags_any.join("|")));
        }
        if let Some(min_confidence) = self.confidence_at_least {
            parts.push(format!("confidence>={:.2}", min_confidence));
        }
        if let Some(max_confidence) = self.confidence_at_most {
            parts.push(format!("confidence<={:.2}", max_confidence));
        }
        if let Some(fix_effort) = &self.fix_effort {
            let fix_effort = fix_effort.trim();
            if !fix_effort.is_empty() {
                parts.push(format!("fix_effort={}", fix_effort));
            }
        }
        if let Some(rule_id) = &self.rule_id {
            let rule_id = rule_id.trim();
            if !rule_id.is_empty() {
                if self.require_rule_id {
                    parts.push(format!("rule_id={} (required)", rule_id));
                } else {
                    parts.push(format!("rule_id={} (label)", rule_id));
                }
            }
        }

        if parts.is_empty() {
            "empty-pattern".to_string()
        } else {
            parts.join(", ")
        }
    }
}
