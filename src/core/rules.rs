use anyhow::Result;
use glob::glob;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewRule {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RuleFileFormat {
    Wrapped { rules: Vec<RuleSpec> },
    List(Vec<RuleSpec>),
}

#[derive(Debug, Clone, Deserialize)]
struct RuleSpec {
    id: String,
    description: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

pub fn load_rules_from_patterns(
    base_path: &Path,
    patterns: &[String],
    source: &str,
    max_rules: usize,
) -> Result<Vec<ReviewRule>> {
    if patterns.is_empty() || max_rules == 0 {
        return Ok(Vec::new());
    }

    let mut matched_files = HashSet::new();
    for pattern in patterns {
        let pattern_path = if Path::new(pattern).is_absolute() {
            pattern.clone()
        } else {
            base_path.join(pattern).to_string_lossy().to_string()
        };

        let entries = match glob(&pattern_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            if entry.is_file() {
                matched_files.insert(entry);
            }
        }
    }

    let mut all_rules = Vec::new();
    for file_path in matched_files {
        if all_rules.len() >= max_rules {
            break;
        }
        if !is_rule_file(&file_path) {
            continue;
        }

        let content = match std::fs::read_to_string(&file_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let mut parsed = parse_rule_file(&content, source)?;
        all_rules.append(&mut parsed);
    }

    all_rules.truncate(max_rules);
    Ok(all_rules)
}

pub fn active_rules_for_file(
    rules: &[ReviewRule],
    file_path: &Path,
    max_active_rules: usize,
) -> Vec<ReviewRule> {
    let file_path_str = file_path.to_string_lossy();
    rules
        .iter()
        .filter(|rule| match rule.scope.as_deref() {
            Some(scope) => path_matches(&file_path_str, scope),
            None => true,
        })
        .take(max_active_rules)
        .cloned()
        .collect()
}

fn is_rule_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("yml" | "yaml" | "json")
    )
}

fn parse_rule_file(content: &str, source: &str) -> Result<Vec<ReviewRule>> {
    let parsed: RuleFileFormat = match serde_yaml::from_str(content) {
        Ok(value) => value,
        Err(_) => serde_json::from_str(content)?,
    };

    let specs = match parsed {
        RuleFileFormat::Wrapped { rules } => rules,
        RuleFileFormat::List(rules) => rules,
    };

    let mut out = Vec::new();
    for mut spec in specs {
        spec.id = spec.id.trim().to_string();
        spec.description = spec.description.trim().to_string();
        if spec.id.is_empty() || spec.description.is_empty() {
            continue;
        }
        let scope = spec.scope.and_then(|scope| {
            let trimmed = scope.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let severity = spec.severity.and_then(|severity| {
            let trimmed = severity.trim().to_lowercase();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let category = spec.category.and_then(|category| {
            let trimmed = category.trim().to_lowercase();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let tags = spec
            .tags
            .into_iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();

        out.push(ReviewRule {
            id: spec.id,
            description: spec.description,
            scope,
            severity,
            category,
            tags,
            source: source.to_string(),
        });
    }

    Ok(out)
}

fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        glob::Pattern::new(pattern)
            .map(|pattern| pattern.matches(path))
            .unwrap_or(false)
    } else {
        path.starts_with(pattern)
    }
}
