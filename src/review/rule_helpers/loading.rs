use std::collections::HashSet;
use std::path::Path;
use tracing::{info, warn};

use crate::config;
use crate::core;

use super::super::context_helpers::PatternRepositoryMap;

pub fn load_review_rules(
    config: &config::Config,
    resolved_repositories: &PatternRepositoryMap,
    repo_root: &Path,
) -> Vec<core::ReviewRule> {
    let mut rules = Vec::new();
    let local_patterns = if config.rules_files.is_empty() {
        vec![
            ".diffscope-rules.yml".to_string(),
            ".diffscope-rules.yaml".to_string(),
            ".diffscope-rules.json".to_string(),
            "rules/**/*.yml".to_string(),
            "rules/**/*.yaml".to_string(),
            "rules/**/*.json".to_string(),
        ]
    } else {
        config.rules_files.clone()
    };

    let local_max_rules = config.max_active_rules.saturating_mul(8).max(64);
    match core::load_rules_from_patterns(repo_root, &local_patterns, "repository", local_max_rules)
    {
        Ok(mut loaded) => rules.append(&mut loaded),
        Err(err) => warn!("Failed to load repository rules: {}", err),
    }

    for repo in &config.pattern_repositories {
        if repo.rule_patterns.is_empty() {
            continue;
        }
        let Some(base_path) = resolved_repositories.get(&repo.source) else {
            continue;
        };

        let max_rules = repo.max_rules.max(config.max_active_rules);
        match core::load_rules_from_patterns(
            base_path,
            &repo.rule_patterns,
            &repo.source,
            max_rules,
        ) {
            Ok(mut loaded) => rules.append(&mut loaded),
            Err(err) => warn!(
                "Failed to load pattern repository rules from '{}': {}",
                repo.source, err
            ),
        }
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for rule in rules {
        let key = rule.id.trim().to_ascii_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        unique.push(rule);
    }

    if !unique.is_empty() {
        info!("Loaded {} review rule(s)", unique.len());
    }
    unique
}
