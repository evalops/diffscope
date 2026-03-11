use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;

/// Identifies the role a model plays in the review pipeline.
///
/// Different tasks benefit from different model tiers: cheap/fast models
/// for triage and summarization, frontier models for deep review, and
/// specialised models for reasoning or embeddings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    /// The main review model (default).
    Primary,
    /// Cheap/fast model for triage, summarization, NL translation.
    Weak,
    /// Reasoning-capable model for complex analysis and self-reflection.
    Reasoning,
    /// Embedding model for RAG indexing.
    Embedding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,

    /// Cheap/fast model for triage, summarization, NL translation.
    #[serde(default)]
    pub model_weak: Option<String>,

    /// Reasoning-capable model for complex analysis and self-reflection.
    #[serde(default)]
    pub model_reasoning: Option<String>,

    /// Embedding model for RAG indexing.
    #[serde(default)]
    pub model_embedding: Option<String>,

    /// Fallback models tried in order when the primary model fails.
    #[serde(default)]
    pub fallback_models: Vec<String>,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    #[serde(default = "default_max_context_chars")]
    pub max_context_chars: usize,

    #[serde(default = "default_max_diff_chars")]
    pub max_diff_chars: usize,

    #[serde(default = "default_context_max_chunks")]
    pub context_max_chunks: usize,

    #[serde(default = "default_context_budget_chars")]
    pub context_budget_chars: usize,

    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,

    #[serde(default = "default_strictness")]
    pub strictness: u8,

    #[serde(default = "default_comment_types")]
    pub comment_types: Vec<String>,

    #[serde(default)]
    pub review_profile: Option<String>,

    #[serde(default)]
    pub review_instructions: Option<String>,

    #[serde(default = "default_true")]
    pub smart_review_summary: bool,

    #[serde(default)]
    pub smart_review_diagram: bool,

    #[serde(default = "default_true")]
    pub symbol_index: bool,

    #[serde(default = "default_symbol_index_provider")]
    pub symbol_index_provider: String,

    #[serde(default = "default_symbol_index_max_files")]
    pub symbol_index_max_files: usize,

    #[serde(default = "default_symbol_index_max_bytes")]
    pub symbol_index_max_bytes: usize,

    #[serde(default = "default_symbol_index_max_locations")]
    pub symbol_index_max_locations: usize,

    #[serde(default = "default_symbol_index_graph_hops")]
    pub symbol_index_graph_hops: usize,

    #[serde(default = "default_symbol_index_graph_max_files")]
    pub symbol_index_graph_max_files: usize,

    #[serde(default)]
    pub symbol_index_lsp_command: Option<String>,

    #[serde(default = "default_symbol_index_lsp_languages")]
    pub symbol_index_lsp_languages: HashMap<String, String>,

    #[serde(default = "default_feedback_path")]
    pub feedback_path: PathBuf,

    /// Path to the convention store file for learned review patterns.
    /// Defaults to ~/.local/share/diffscope/conventions.json if not set.
    #[serde(default)]
    pub convention_store_path: Option<String>,

    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,

    #[serde(default)]
    pub adapter: Option<String>,

    #[serde(default)]
    pub context_window: Option<usize>,

    #[serde(default)]
    pub openai_use_responses: Option<bool>,

    /// HTTP timeout in seconds for LLM adapter requests.
    /// Defaults: 60s for cloud APIs, 300s for local endpoints.
    #[serde(default)]
    pub adapter_timeout_secs: Option<u64>,

    /// Maximum number of retries on transient failures (429, 5xx).
    #[serde(default)]
    pub adapter_max_retries: Option<usize>,

    /// Base delay in milliseconds between retries (linear backoff).
    #[serde(default)]
    pub adapter_retry_delay_ms: Option<u64>,

    /// Maximum number of file changes before skipping review (0 = no limit).
    #[serde(default)]
    pub file_change_limit: Option<usize>,

    /// Auto-detect and absorb .cursorrules, CLAUDE.md, agents.md files.
    #[serde(default = "default_true")]
    pub auto_detect_instructions: bool,

    /// Language/locale for review output (e.g., "en", "ja", "de").
    #[serde(default)]
    pub output_language: Option<String>,

    /// Whether to include AI fix suggestions with comments.
    #[serde(default = "default_true")]
    pub include_fix_suggestions: bool,

    /// Minimum number of rejections before adaptive suppression kicks in.
    #[serde(default = "default_feedback_suppression_threshold")]
    pub feedback_suppression_threshold: usize,

    /// Margin: rejected must exceed accepted by this amount for suppression.
    #[serde(default = "default_feedback_suppression_margin")]
    pub feedback_suppression_margin: usize,

    /// HashiCorp Vault server address (e.g., https://vault.example.com:8200).
    #[serde(default)]
    pub vault_addr: Option<String>,

    /// Vault authentication token.
    #[serde(default)]
    pub vault_token: Option<String>,

    /// Secret path in Vault (e.g., "diffscope" or "ci/diffscope").
    #[serde(default)]
    pub vault_path: Option<String>,

    /// Key within the Vault secret to extract as the API key (default: "api_key").
    #[serde(default)]
    pub vault_key: Option<String>,

    /// Vault KV engine mount point (default: "secret").
    #[serde(default)]
    pub vault_mount: Option<String>,

    /// Vault Enterprise namespace.
    #[serde(default)]
    pub vault_namespace: Option<String>,

    #[serde(default)]
    pub plugins: PluginConfig,

    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,

    #[serde(default)]
    pub paths: HashMap<String, PathConfig>,

    #[serde(default)]
    pub custom_context: Vec<CustomContextConfig>,

    #[serde(default)]
    pub pattern_repositories: Vec<PatternRepositoryConfig>,

    #[serde(default)]
    pub rules_files: Vec<String>,

    #[serde(default = "default_max_active_rules")]
    pub max_active_rules: usize,

    #[serde(default)]
    pub rule_priority: Vec<String>,

    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,

    #[serde(default)]
    pub github_token: Option<String>,

    /// GitHub App ID (from app settings page).
    #[serde(default)]
    pub github_app_id: Option<u64>,

    /// GitHub App OAuth client ID (for device flow auth).
    #[serde(default)]
    pub github_client_id: Option<String>,

    /// GitHub App OAuth client secret.
    #[serde(default)]
    pub github_client_secret: Option<String>,

    /// GitHub App private key (PEM content).
    #[serde(default)]
    pub github_private_key: Option<String>,

    /// Webhook secret for verifying GitHub webhook signatures.
    #[serde(default)]
    pub github_webhook_secret: Option<String>,

    /// When true, run separate specialized LLM passes for security, correctness,
    /// and style instead of a single monolithic review prompt.
    #[serde(default = "default_false")]
    pub multi_pass_specialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathConfig {
    #[serde(default)]
    pub focus: Vec<String>,

    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    #[serde(default)]
    pub extra_context: Vec<String>,

    pub system_prompt: Option<String>,

    pub review_instructions: Option<String>,

    #[serde(default)]
    pub severity_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomContextConfig {
    #[serde(default)]
    pub scope: Option<String>,

    #[serde(default)]
    pub notes: Vec<String>,

    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatternRepositoryConfig {
    pub source: String,

    #[serde(default)]
    pub scope: Option<String>,

    #[serde(default)]
    pub include_patterns: Vec<String>,

    #[serde(default = "default_pattern_repo_max_files")]
    pub max_files: usize,

    #[serde(default = "default_pattern_repo_max_lines")]
    pub max_lines: usize,

    #[serde(default)]
    pub rule_patterns: Vec<String>,

    #[serde(default = "default_pattern_repo_max_rules")]
    pub max_rules: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginConfig {
    #[serde(default = "default_true")]
    pub eslint: bool,

    #[serde(default = "default_true")]
    pub semgrep: bool,

    #[serde(default = "default_true")]
    pub duplicate_filter: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: default_model(),
            model_weak: None,
            model_reasoning: None,
            model_embedding: None,
            fallback_models: Vec::new(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            max_context_chars: default_max_context_chars(),
            max_diff_chars: default_max_diff_chars(),
            context_max_chunks: default_context_max_chunks(),
            context_budget_chars: default_context_budget_chars(),
            min_confidence: default_min_confidence(),
            strictness: default_strictness(),
            comment_types: default_comment_types(),
            review_profile: None,
            review_instructions: None,
            smart_review_summary: true,
            smart_review_diagram: false,
            symbol_index: true,
            symbol_index_provider: default_symbol_index_provider(),
            symbol_index_max_files: default_symbol_index_max_files(),
            symbol_index_max_bytes: default_symbol_index_max_bytes(),
            symbol_index_max_locations: default_symbol_index_max_locations(),
            symbol_index_graph_hops: default_symbol_index_graph_hops(),
            symbol_index_graph_max_files: default_symbol_index_graph_max_files(),
            symbol_index_lsp_command: None,
            symbol_index_lsp_languages: default_symbol_index_lsp_languages(),
            feedback_path: default_feedback_path(),
            convention_store_path: None,
            system_prompt: None,
            api_key: None,
            base_url: None,
            adapter: None,
            context_window: None,
            openai_use_responses: None,
            adapter_timeout_secs: None,
            adapter_max_retries: None,
            adapter_retry_delay_ms: None,
            file_change_limit: None,
            auto_detect_instructions: true,
            output_language: None,
            include_fix_suggestions: true,
            feedback_suppression_threshold: default_feedback_suppression_threshold(),
            feedback_suppression_margin: default_feedback_suppression_margin(),
            vault_addr: None,
            vault_token: None,
            vault_path: None,
            vault_key: None,
            vault_mount: None,
            vault_namespace: None,
            plugins: PluginConfig::default(),
            exclude_patterns: default_exclude_patterns(),
            paths: HashMap::new(),
            custom_context: Vec::new(),
            pattern_repositories: Vec::new(),
            rules_files: Vec::new(),
            max_active_rules: default_max_active_rules(),
            rule_priority: Vec::new(),
            providers: HashMap::new(),
            github_token: None,
            github_app_id: None,
            github_client_id: None,
            github_client_secret: None,
            github_private_key: None,
            github_webhook_secret: None,
            multi_pass_specialized: false,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        // Try to load from .diffscope.yml in current directory
        let config_path = PathBuf::from(".diffscope.yml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = serde_yaml::from_str(&content)?;
            return Ok(config);
        }

        // Try alternative names
        let alt_config_path = PathBuf::from(".diffscope.yaml");
        if alt_config_path.exists() {
            let content = std::fs::read_to_string(&alt_config_path)?;
            let config: Config = serde_yaml::from_str(&content)?;
            return Ok(config);
        }

        // Try in home directory
        if let Some(home_dir) = dirs::home_dir() {
            let home_config = home_dir.join(".diffscope.yml");
            if home_config.exists() {
                let content = std::fs::read_to_string(&home_config)?;
                let config: Config = serde_yaml::from_str(&content)?;
                return Ok(config);
            }
        }

        // Return default config if no file found
        Ok(Config::default())
    }
}

/// CLI overrides collected from command-line arguments.
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
    pub strictness: Option<u8>,
    pub comment_types: Option<Vec<String>>,
    pub openai_responses: Option<bool>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub adapter: Option<String>,
    pub lsp_command: Option<String>,
    pub timeout: Option<u64>,
    pub max_retries: Option<usize>,
    pub file_change_limit: Option<usize>,
    pub output_language: Option<String>,
    pub vault_addr: Option<String>,
    pub vault_path: Option<String>,
    pub vault_key: Option<String>,
}

impl Config {
    pub fn merge_with_cli(&mut self, cli_model: Option<String>, cli_prompt: Option<String>) {
        if let Some(model) = cli_model {
            self.model = model;
        }
        if let Some(prompt) = cli_prompt {
            self.system_prompt = Some(prompt);
        }
    }

    /// Apply CLI overrides to config. Only overrides fields that are Some/provided.
    pub fn apply_cli_overrides(&mut self, cli: CliOverrides) {
        if let Some(v) = cli.temperature {
            self.temperature = v;
        }
        if let Some(v) = cli.max_tokens {
            self.max_tokens = v;
        }
        if let Some(v) = cli.strictness {
            self.strictness = v;
        }
        if let Some(v) = cli.comment_types {
            self.comment_types = v;
        }
        if let Some(v) = cli.openai_responses {
            self.openai_use_responses = Some(v);
        }
        if let Some(v) = cli.base_url {
            self.base_url = Some(v);
        }
        if let Some(v) = cli.api_key {
            self.api_key = Some(v);
        }
        if let Some(v) = cli.adapter {
            self.adapter = Some(v);
        }
        if let Some(command) = cli.lsp_command {
            self.symbol_index = true;
            self.symbol_index_provider = "lsp".to_string();
            self.symbol_index_lsp_command = Some(command);
        }
        if let Some(v) = cli.timeout {
            self.adapter_timeout_secs = Some(v);
        }
        if let Some(v) = cli.max_retries {
            self.adapter_max_retries = Some(v);
        }
        if let Some(v) = cli.file_change_limit {
            self.file_change_limit = Some(v);
        }
        if let Some(v) = cli.output_language {
            self.output_language = Some(v);
        }
        if let Some(v) = cli.vault_addr {
            self.vault_addr = Some(v);
        }
        if let Some(v) = cli.vault_path {
            self.vault_path = Some(v);
        }
        if let Some(v) = cli.vault_key {
            self.vault_key = Some(v);
        }
    }

    pub fn normalize(&mut self) {
        // Env var fallbacks for base_url and api_key
        if self.base_url.is_none() {
            self.base_url = std::env::var("DIFFSCOPE_BASE_URL")
                .ok()
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .filter(|s| !s.trim().is_empty());
        }
        if self.api_key.is_none() {
            self.api_key = std::env::var("DIFFSCOPE_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty());
        }

        // Env var fallbacks for GitHub integration
        if self.github_token.is_none() {
            self.github_token = std::env::var("GITHUB_TOKEN")
                .ok()
                .filter(|s| !s.trim().is_empty());
        }
        if self.github_webhook_secret.is_none() {
            self.github_webhook_secret = std::env::var("DIFFSCOPE_WEBHOOK_SECRET")
                .ok()
                .filter(|s| !s.trim().is_empty());
        }

        // Validate base_url: must be a valid http/https URL with a host
        if let Some(ref raw_url) = self.base_url {
            match url::Url::parse(raw_url) {
                Ok(parsed) => {
                    if !matches!(parsed.scheme(), "http" | "https") {
                        warn!(
                            "base_url '{}' uses unsupported scheme '{}' (expected http or https), ignoring",
                            raw_url,
                            parsed.scheme()
                        );
                        self.base_url = None;
                    } else if parsed.host().is_none() {
                        warn!("base_url '{}' has no valid host, ignoring", raw_url);
                        self.base_url = None;
                    }
                }
                Err(err) => {
                    warn!(
                        "base_url '{}' is not a valid URL ({}), ignoring",
                        raw_url, err
                    );
                    self.base_url = None;
                }
            }
        }

        // Normalize adapter field
        if let Some(ref adapter) = self.adapter {
            let normalized = adapter.trim().to_lowercase();
            self.adapter = if matches!(normalized.as_str(), "openai" | "anthropic" | "ollama") {
                Some(normalized)
            } else {
                None
            };
        }

        if self.model.trim().is_empty() {
            self.model = default_model();
        }

        if !self.temperature.is_finite() || self.temperature < 0.0 || self.temperature > 2.0 {
            warn!(
                "temperature {} is outside valid range 0.0..=2.0, resetting to default {}",
                self.temperature,
                default_temperature()
            );
            self.temperature = default_temperature();
        }

        if self.max_tokens == 0 {
            warn!(
                "max_tokens is 0, resetting to default {}",
                default_max_tokens()
            );
            self.max_tokens = default_max_tokens();
        } else if self.max_tokens > 128_000 {
            warn!(
                "max_tokens {} exceeds maximum 128000, clamping to 128000",
                self.max_tokens
            );
            self.max_tokens = 128_000;
        }
        if self.context_max_chunks == 0 {
            self.context_max_chunks = default_context_max_chunks();
        }
        if self.context_budget_chars == 0 {
            self.context_budget_chars = default_context_budget_chars();
        }

        if self.symbol_index_max_files == 0 {
            self.symbol_index_max_files = default_symbol_index_max_files();
        }
        if self.symbol_index_max_bytes == 0 {
            self.symbol_index_max_bytes = default_symbol_index_max_bytes();
        }
        if self.symbol_index_max_locations == 0 {
            self.symbol_index_max_locations = default_symbol_index_max_locations();
        }
        if self.symbol_index_graph_hops == 0 {
            self.symbol_index_graph_hops = default_symbol_index_graph_hops();
        }
        if self.symbol_index_graph_max_files == 0 {
            self.symbol_index_graph_max_files = default_symbol_index_graph_max_files();
        }

        let provider = self.symbol_index_provider.trim().to_lowercase();
        if provider.is_empty() || !matches!(provider.as_str(), "regex" | "lsp") {
            self.symbol_index_provider = default_symbol_index_provider();
        } else {
            self.symbol_index_provider = provider;
        }

        if let Some(command) = &self.symbol_index_lsp_command {
            if command.trim().is_empty() {
                self.symbol_index_lsp_command = None;
            }
        }

        if self.symbol_index_provider == "lsp" && self.symbol_index_lsp_languages.is_empty() {
            self.symbol_index_lsp_languages = default_symbol_index_lsp_languages();
        }

        if !self.min_confidence.is_finite() {
            self.min_confidence = default_min_confidence();
        } else if !(0.0..=1.0).contains(&self.min_confidence) {
            self.min_confidence = self.min_confidence.clamp(0.0, 1.0);
        }
        if self.strictness == 0 {
            warn!(
                "strictness 0 is invalid (valid range: 1-3), resetting to default {}",
                default_strictness()
            );
            self.strictness = default_strictness();
        } else if self.strictness > 3 {
            warn!(
                "strictness {} is invalid (valid range: 1-3), clamping to 3",
                self.strictness
            );
            self.strictness = 3;
        }

        self.comment_types = normalize_comment_types(&self.comment_types);

        if let Some(profile) = &self.review_profile {
            let normalized = profile.trim().to_lowercase();
            self.review_profile = if normalized.is_empty() {
                None
            } else if matches!(normalized.as_str(), "balanced" | "chill" | "assertive") {
                Some(normalized)
            } else {
                None
            };
        }

        if let Some(instructions) = &self.review_instructions {
            if instructions.trim().is_empty() {
                self.review_instructions = None;
            }
        }

        let mut normalized_custom_context = Vec::new();
        for mut entry in std::mem::take(&mut self.custom_context) {
            entry.scope = entry.scope.and_then(|scope| {
                let trimmed = scope.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });

            entry.notes = entry
                .notes
                .into_iter()
                .map(|note| note.trim().to_string())
                .filter(|note| !note.is_empty())
                .collect();
            entry.files = entry
                .files
                .into_iter()
                .map(|file| file.trim().to_string())
                .filter(|file| !file.is_empty())
                .collect();

            if entry.notes.is_empty() && entry.files.is_empty() {
                continue;
            }
            normalized_custom_context.push(entry);
        }
        self.custom_context = normalized_custom_context;

        let mut normalized_pattern_repositories = Vec::new();
        for mut repo in std::mem::take(&mut self.pattern_repositories) {
            repo.source = repo.source.trim().to_string();
            if repo.source.is_empty() {
                continue;
            }
            repo.scope = repo.scope.and_then(|scope| {
                let trimmed = scope.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });
            repo.include_patterns = repo
                .include_patterns
                .into_iter()
                .map(|pattern| pattern.trim().to_string())
                .filter(|pattern| !pattern.is_empty())
                .collect();
            if repo.include_patterns.is_empty() {
                repo.include_patterns.push("**/*".to_string());
            }
            if repo.max_files == 0 {
                repo.max_files = default_pattern_repo_max_files();
            }
            if repo.max_lines == 0 {
                repo.max_lines = default_pattern_repo_max_lines();
            }
            if repo.max_rules == 0 {
                repo.max_rules = default_pattern_repo_max_rules();
            }
            repo.rule_patterns = repo
                .rule_patterns
                .into_iter()
                .map(|pattern| pattern.trim().to_string())
                .filter(|pattern| !pattern.is_empty())
                .collect();

            normalized_pattern_repositories.push(repo);
        }
        self.pattern_repositories = normalized_pattern_repositories;

        self.rules_files = self
            .rules_files
            .iter()
            .map(|pattern| pattern.trim().to_string())
            .filter(|pattern| !pattern.is_empty())
            .collect();
        if self.max_active_rules == 0 {
            self.max_active_rules = default_max_active_rules();
        }
        self.rule_priority = self
            .rule_priority
            .iter()
            .map(|rule| rule.trim().to_ascii_lowercase())
            .filter(|rule| !rule.is_empty())
            .fold(Vec::new(), |mut acc, rule| {
                if !acc.contains(&rule) {
                    acc.push(rule);
                }
                acc
            });

        // Clamp adapter timeout to reasonable range (5s - 600s)
        if let Some(timeout) = self.adapter_timeout_secs {
            if timeout == 0 {
                self.adapter_timeout_secs = None; // use default
            } else {
                self.adapter_timeout_secs = Some(timeout.clamp(5, 600));
            }
        }
        // Clamp adapter retries (0-10)
        if let Some(retries) = self.adapter_max_retries {
            self.adapter_max_retries = Some(retries.min(10));
        }
        // Clamp retry delay (50ms - 30s)
        if let Some(delay) = self.adapter_retry_delay_ms {
            if delay == 0 {
                self.adapter_retry_delay_ms = None;
            } else {
                self.adapter_retry_delay_ms = Some(delay.clamp(50, 30_000));
            }
        }
        // Normalize output language
        if let Some(ref lang) = self.output_language {
            let trimmed = lang.trim().to_lowercase();
            self.output_language = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
        }
        // Ensure suppression thresholds are reasonable
        if self.feedback_suppression_threshold == 0 {
            self.feedback_suppression_threshold = default_feedback_suppression_threshold();
        }
    }

    pub fn get_path_config(&self, file_path: &Path) -> Option<&PathConfig> {
        let file_path_str = file_path.to_string_lossy();

        // Find the most specific matching path
        let mut best_match: Option<(&String, &PathConfig)> = None;

        for (pattern, config) in &self.paths {
            if self.path_matches(&file_path_str, pattern) {
                // Keep the most specific match (longest pattern)
                if best_match.is_none() || pattern.len() > best_match.unwrap().0.len() {
                    best_match = Some((pattern, config));
                }
            }
        }

        best_match.map(|(_, config)| config)
    }

    pub fn should_exclude(&self, file_path: &Path) -> bool {
        let file_path_str = file_path.to_string_lossy();

        // Check global exclude patterns
        for pattern in &self.exclude_patterns {
            if self.path_matches(&file_path_str, pattern) {
                return true;
            }
        }

        // Check path-specific ignore patterns
        if let Some(path_config) = self.get_path_config(file_path) {
            for pattern in &path_config.ignore_patterns {
                if self.path_matches(&file_path_str, pattern) {
                    return true;
                }
            }
        }

        false
    }

    pub fn matching_custom_context(&self, file_path: &Path) -> Vec<&CustomContextConfig> {
        let file_path_str = file_path.to_string_lossy();
        self.custom_context
            .iter()
            .filter(|entry| match entry.scope.as_deref() {
                Some(scope) => self.path_matches(&file_path_str, scope),
                None => true,
            })
            .collect()
    }

    pub fn effective_min_confidence(&self) -> f32 {
        let strictness_floor = match self.strictness {
            1 => 0.85,
            2 => 0.65,
            _ => 0.45,
        };
        self.min_confidence.max(strictness_floor).clamp(0.0, 1.0)
    }

    pub fn matching_pattern_repositories(&self, file_path: &Path) -> Vec<&PatternRepositoryConfig> {
        let file_path_str = file_path.to_string_lossy();
        self.pattern_repositories
            .iter()
            .filter(|repo| match repo.scope.as_deref() {
                Some(scope) => self.path_matches(&file_path_str, scope),
                None => true,
            })
            .collect()
    }

    /// Build a ModelConfig from this Config.
    pub fn to_model_config(&self) -> crate::adapters::llm::ModelConfig {
        crate::adapters::llm::ModelConfig {
            model_name: self.model.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            openai_use_responses: self.openai_use_responses,
            adapter_override: self.adapter.clone(),
            timeout_secs: self.adapter_timeout_secs,
            max_retries: self.adapter_max_retries,
            retry_delay_ms: self.adapter_retry_delay_ms,
        }
    }

    /// Get the model name for a specific role, falling back to the primary model.
    pub fn model_for_role(&self, role: ModelRole) -> &str {
        match role {
            ModelRole::Primary => &self.model,
            ModelRole::Weak => self.model_weak.as_deref().unwrap_or(&self.model),
            ModelRole::Reasoning => self.model_reasoning.as_deref().unwrap_or(&self.model),
            ModelRole::Embedding => self.model_embedding.as_deref().unwrap_or(&self.model),
        }
    }

    /// Build a ModelConfig for a specific role.
    pub fn to_model_config_for_role(&self, role: ModelRole) -> crate::adapters::llm::ModelConfig {
        let mut config = self.to_model_config();
        config.model_name = self.model_for_role(role).to_string();
        config
    }

    /// Resolve which provider to use based on configuration.
    ///
    /// Returns `(api_key, base_url, adapter)` by checking:
    /// 1. If `adapter` is explicitly set and a matching enabled provider exists, use it.
    /// 2. If no adapter is set, infer from the model name.
    /// 3. Fall back to top-level `api_key`/`base_url`.
    #[allow(dead_code)]
    pub fn resolve_provider(&self) -> (Option<String>, Option<String>, Option<String>) {
        // If adapter is explicitly set, look for a matching provider
        if let Some(ref adapter) = self.adapter {
            let key = adapter.to_lowercase();
            if let Some(provider) = self.providers.get(&key) {
                if provider.enabled {
                    let api_key = provider.api_key.clone().or_else(|| self.api_key.clone());
                    let base_url = provider.base_url.clone().or_else(|| self.base_url.clone());
                    return (api_key, base_url, Some(key));
                }
            }
            // Adapter is set but no matching provider found; fall through to top-level
            return (self.api_key.clone(), self.base_url.clone(), Some(key));
        }

        // No adapter set: try to detect provider from model name
        let model_lower = self.model.to_lowercase();
        let detected = if model_lower.starts_with("anthropic/") || model_lower.starts_with("claude")
        {
            Some("anthropic")
        } else if model_lower.starts_with("openai/")
            || model_lower.starts_with("gpt")
            || model_lower.starts_with("o1")
            || model_lower.starts_with("o3")
            || model_lower.starts_with("o4")
        {
            Some("openai")
        } else if model_lower.starts_with("ollama:") {
            Some("ollama")
        } else {
            // Default: check if openrouter provider is configured
            if self.providers.get("openrouter").is_some_and(|p| p.enabled) {
                Some("openrouter")
            } else {
                None
            }
        };

        if let Some(provider_key) = detected {
            if let Some(provider) = self.providers.get(provider_key) {
                if provider.enabled {
                    let api_key = provider.api_key.clone().or_else(|| self.api_key.clone());
                    let base_url = provider.base_url.clone().or_else(|| self.base_url.clone());
                    // Map openrouter to openai adapter (OpenRouter uses OpenAI-compatible API)
                    let adapter = match provider_key {
                        "openrouter" => Some("openai".to_string()),
                        other => Some(other.to_string()),
                    };
                    return (api_key, base_url, adapter);
                }
            }
        }

        // Fall back to top-level fields
        (
            self.api_key.clone(),
            self.base_url.clone(),
            self.adapter.clone(),
        )
    }

    /// Try to resolve the API key from Vault if Vault is configured and api_key is not set.
    pub async fn resolve_vault_api_key(&mut self) -> Result<()> {
        if self.api_key.is_some() {
            return Ok(());
        }

        let vault_config = crate::vault::try_build_vault_config(
            self.vault_addr.as_deref(),
            self.vault_token.as_deref(),
            self.vault_path.as_deref(),
            self.vault_key.as_deref(),
            self.vault_mount.as_deref(),
            self.vault_namespace.as_deref(),
        );

        if let Some(vc) = vault_config {
            tracing::info!("Fetching API key from Vault at {}", vc.addr);
            let secret = crate::vault::fetch_secret(&vc).await?;
            self.api_key = Some(secret);
            tracing::info!("API key loaded from Vault");
        }

        Ok(())
    }

    /// Returns true if the configured base_url points to a local/self-hosted server.
    pub fn is_local_endpoint(&self) -> bool {
        match self.base_url.as_deref() {
            Some(url) => {
                url.contains("localhost")
                    || url.contains("127.0.0.1")
                    || url.contains("0.0.0.0")
                    || url.contains("[::1]")
                    || (!url.contains("openai.com") && !url.contains("anthropic.com"))
            }
            None => false,
        }
    }

    fn path_matches(&self, path: &str, pattern: &str) -> bool {
        // Simple glob matching
        if pattern.contains('*') {
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                glob_pattern.matches(path)
            } else {
                false
            }
        } else {
            // Path prefix matching with component boundary check
            path == pattern || path.starts_with(&format!("{}/", pattern.trim_end_matches('/')))
        }
    }
}

fn default_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_temperature() -> f32 {
    0.2
}

fn default_max_tokens() -> usize {
    4000
}

fn default_max_context_chars() -> usize {
    20000
}

fn default_max_diff_chars() -> usize {
    40000
}

fn default_exclude_patterns() -> Vec<String> {
    [
        "*.lock",
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "go.sum",
        "Gemfile.lock",
        "Pipfile.lock",
        "poetry.lock",
        "composer.lock",
        "*.min.js",
        "*.min.css",
        "*.map",
        "*.generated.*",
        "*.pb.go",
        "*.pb.rs",
        "*_generated.go",
        "vendor/**",
        "node_modules/**",
        ".git/**",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_context_max_chunks() -> usize {
    24
}

fn default_context_budget_chars() -> usize {
    24000
}

fn default_min_confidence() -> f32 {
    0.0
}

fn default_strictness() -> u8 {
    2
}

fn default_comment_types() -> Vec<String> {
    vec![
        "logic".to_string(),
        "syntax".to_string(),
        "style".to_string(),
        "informational".to_string(),
    ]
}

fn default_symbol_index_max_files() -> usize {
    500
}

fn default_symbol_index_max_bytes() -> usize {
    200_000
}

fn default_symbol_index_max_locations() -> usize {
    5
}

fn default_symbol_index_graph_hops() -> usize {
    2
}

fn default_symbol_index_graph_max_files() -> usize {
    12
}

fn default_symbol_index_provider() -> String {
    "regex".to_string()
}

fn default_symbol_index_lsp_languages() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("rs".to_string(), "rust".to_string());
    map
}

fn default_feedback_path() -> PathBuf {
    PathBuf::from(".diffscope.feedback.json")
}

fn default_pattern_repo_max_files() -> usize {
    8
}

fn default_pattern_repo_max_lines() -> usize {
    200
}

fn default_pattern_repo_max_rules() -> usize {
    200
}

fn default_max_active_rules() -> usize {
    30
}

fn default_feedback_suppression_threshold() -> usize {
    3
}

fn default_feedback_suppression_margin() -> usize {
    2
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn normalize_comment_types(values: &[String]) -> Vec<String> {
    if values.is_empty() {
        return default_comment_types();
    }

    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().to_lowercase();
        if !matches!(
            value.as_str(),
            "logic" | "syntax" | "style" | "informational"
        ) {
            continue;
        }
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    if normalized.is_empty() {
        default_comment_types()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_clamps_values() {
        let mut config = Config {
            model: "   ".to_string(),
            temperature: 5.0,
            max_tokens: 0,
            min_confidence: 2.0,
            strictness: 0,
            review_profile: Some("ASSERTIVE".to_string()),
            ..Config::default()
        };

        config.normalize();

        assert_eq!(config.model, default_model());
        assert_eq!(config.temperature, default_temperature());
        assert_eq!(config.max_tokens, default_max_tokens());
        assert_eq!(config.min_confidence, 1.0);
        assert_eq!(config.strictness, default_strictness());
        assert_eq!(config.review_profile.as_deref(), Some("assertive"));
    }

    #[test]
    fn normalize_comment_types_filters_unknown_values() {
        let mut config = Config {
            comment_types: vec![
                " LOGIC ".to_string(),
                "style".to_string(),
                "unknown".to_string(),
                "STYLE".to_string(),
            ],
            ..Config::default()
        };

        config.normalize();

        assert_eq!(config.comment_types, vec!["logic", "style"]);
    }

    #[test]
    fn path_matches_respects_component_boundary() {
        let config = Config::default();

        // Exact prefix with separator should match
        assert!(config.path_matches("src/file.rs", "src/"));
        assert!(config.path_matches("src/sub/file.rs", "src"));

        // Glob patterns should still work
        assert!(config.path_matches("src/file.rs", "src/*.rs"));

        // Non-glob pattern must NOT match a different path component
        // "src" should not match "srcfoo/file.rs" or "src-backup/file.rs"
        assert!(
            !config.path_matches("srcfoo/file.rs", "src"),
            "pattern 'src' should not match 'srcfoo/file.rs' (different path component)"
        );
        assert!(
            !config.path_matches("src-backup/file.rs", "src"),
            "pattern 'src' should not match 'src-backup/file.rs'"
        );

        // Exact match should work
        assert!(config.path_matches("src/file.rs", "src/file.rs"));
    }

    #[test]
    fn normalize_validates_base_url_valid_http() {
        let mut config = Config {
            base_url: Some("http://localhost:11434".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.base_url.as_deref(), Some("http://localhost:11434"));
    }

    #[test]
    fn normalize_validates_base_url_valid_https() {
        let mut config = Config {
            base_url: Some("https://api.openai.com/v1".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
    }

    #[test]
    fn normalize_rejects_base_url_bad_scheme() {
        let mut config = Config {
            base_url: Some("ftp://example.com".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert!(config.base_url.is_none());
    }

    #[test]
    fn normalize_rejects_base_url_no_host() {
        let mut config = Config {
            base_url: Some("http://".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert!(config.base_url.is_none());
    }

    #[test]
    fn normalize_rejects_base_url_not_a_url() {
        let mut config = Config {
            base_url: Some("not a url at all".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert!(config.base_url.is_none());
    }

    #[test]
    fn normalize_rejects_base_url_javascript_scheme() {
        let mut config = Config {
            base_url: Some("javascript:alert(1)".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert!(config.base_url.is_none());
    }

    #[test]
    fn normalize_clamps_max_tokens_above_limit() {
        let mut config = Config {
            max_tokens: 200_000,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.max_tokens, 128_000);
    }

    #[test]
    fn normalize_accepts_max_tokens_at_limit() {
        let mut config = Config {
            max_tokens: 128_000,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.max_tokens, 128_000);
    }

    #[test]
    fn normalize_strictness_warns_and_clamps_above_3() {
        let mut config = Config {
            strictness: 5,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.strictness, 3);
    }

    #[test]
    fn normalize_strictness_warns_and_defaults_zero() {
        let mut config = Config {
            strictness: 0,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.strictness, default_strictness());
    }

    #[test]
    fn normalize_accepts_valid_strictness() {
        for s in 1..=3 {
            let mut config = Config {
                strictness: s,
                ..Config::default()
            };
            config.normalize();
            assert_eq!(config.strictness, s);
        }
    }

    #[test]
    fn normalize_temperature_negative() {
        let mut config = Config {
            temperature: -0.5,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.temperature, default_temperature());
    }

    #[test]
    fn normalize_temperature_nan() {
        let mut config = Config {
            temperature: f32::NAN,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.temperature, default_temperature());
    }

    #[test]
    fn normalize_temperature_infinity() {
        let mut config = Config {
            temperature: f32::INFINITY,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.temperature, default_temperature());
    }

    #[test]
    fn normalize_adapter_timeout_clamps_to_max() {
        let mut config = Config {
            adapter_timeout_secs: Some(9999),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.adapter_timeout_secs, Some(600));
    }

    #[test]
    fn normalize_adapter_timeout_zero_clears() {
        let mut config = Config {
            adapter_timeout_secs: Some(0),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.adapter_timeout_secs, None);
    }

    #[test]
    fn normalize_adapter_retries_clamps() {
        let mut config = Config {
            adapter_max_retries: Some(50),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.adapter_max_retries, Some(10));
    }

    #[test]
    fn normalize_output_language_trims() {
        let mut config = Config {
            output_language: Some("  JA  ".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.output_language.as_deref(), Some("ja"));
    }

    #[test]
    fn normalize_output_language_empty_clears() {
        let mut config = Config {
            output_language: Some("   ".to_string()),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.output_language, None);
    }

    #[test]
    fn normalize_adapter_timeout_clamps_minimum() {
        let mut config = Config {
            adapter_timeout_secs: Some(2),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.adapter_timeout_secs, Some(5));
    }

    #[test]
    fn normalize_adapter_retry_delay_clamps_minimum() {
        let mut config = Config {
            adapter_retry_delay_ms: Some(10),
            ..Config::default()
        };
        config.normalize();
        assert_eq!(config.adapter_retry_delay_ms, Some(50));
    }

    #[test]
    fn normalize_feedback_suppression_zero_resets() {
        let mut config = Config {
            feedback_suppression_threshold: 0,
            ..Config::default()
        };
        config.normalize();
        assert_eq!(
            config.feedback_suppression_threshold,
            default_feedback_suppression_threshold()
        );
    }

    #[test]
    fn test_apply_cli_overrides() {
        let mut config = Config::default();
        config.apply_cli_overrides(CliOverrides {
            temperature: Some(0.5),
            max_tokens: Some(8000),
            strictness: Some(3),
            comment_types: Some(vec!["logic".to_string()]),
            openai_responses: Some(true),
            base_url: Some("http://localhost:1234".to_string()),
            api_key: Some("test-key".to_string()),
            adapter: Some("openai".to_string()),
            timeout: Some(60),
            max_retries: Some(5),
            file_change_limit: Some(10),
            output_language: Some("ja".to_string()),
            ..Default::default()
        });
        assert!((config.temperature - 0.5).abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, 8000);
        assert_eq!(config.strictness, 3);
        assert_eq!(config.comment_types, vec!["logic".to_string()]);
        assert_eq!(config.openai_use_responses, Some(true));
        assert_eq!(config.base_url.as_deref(), Some("http://localhost:1234"));
        assert_eq!(config.api_key.as_deref(), Some("test-key"));
        assert_eq!(config.adapter.as_deref(), Some("openai"));
        assert_eq!(config.adapter_timeout_secs, Some(60));
        assert_eq!(config.adapter_max_retries, Some(5));
        assert_eq!(config.file_change_limit, Some(10));
        assert_eq!(config.output_language.as_deref(), Some("ja"));
    }

    #[test]
    fn test_apply_cli_overrides_nones_dont_change() {
        let mut config = Config::default();
        let orig_temp = config.temperature;
        let orig_tokens = config.max_tokens;
        config.apply_cli_overrides(CliOverrides::default());
        assert!((config.temperature - orig_temp).abs() < f32::EPSILON);
        assert_eq!(config.max_tokens, orig_tokens);
    }

    #[test]
    fn test_apply_cli_overrides_lsp() {
        let mut config = Config::default();
        config.apply_cli_overrides(CliOverrides {
            lsp_command: Some("rust-analyzer".to_string()),
            ..Default::default()
        });
        assert!(config.symbol_index);
        assert_eq!(config.symbol_index_provider, "lsp");
        assert_eq!(
            config.symbol_index_lsp_command.as_deref(),
            Some("rust-analyzer")
        );
    }

    #[test]
    fn test_model_role_primary_returns_model() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            ..Config::default()
        };
        assert_eq!(
            config.model_for_role(ModelRole::Primary),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn test_model_role_weak_fallback_to_primary() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_weak: None,
            ..Config::default()
        };
        assert_eq!(config.model_for_role(ModelRole::Weak), "claude-sonnet-4-6");
    }

    #[test]
    fn test_model_role_weak_explicit() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_weak: Some("claude-haiku-4-5".to_string()),
            ..Config::default()
        };
        assert_eq!(config.model_for_role(ModelRole::Weak), "claude-haiku-4-5");
    }

    #[test]
    fn test_model_role_reasoning_fallback() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_reasoning: None,
            ..Config::default()
        };
        assert_eq!(
            config.model_for_role(ModelRole::Reasoning),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn test_model_role_reasoning_explicit() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_reasoning: Some("claude-opus-4-6".to_string()),
            ..Config::default()
        };
        assert_eq!(
            config.model_for_role(ModelRole::Reasoning),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn test_model_role_embedding_default() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_embedding: None,
            ..Config::default()
        };
        // Falls back to primary model when no embedding model configured
        assert_eq!(
            config.model_for_role(ModelRole::Embedding),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn test_model_role_embedding_explicit() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_embedding: Some("custom-embedding-model".to_string()),
            ..Config::default()
        };
        assert_eq!(
            config.model_for_role(ModelRole::Embedding),
            "custom-embedding-model"
        );
    }

    #[test]
    fn test_to_model_config_for_role_uses_correct_model() {
        let config = Config {
            model: "claude-sonnet-4-6".to_string(),
            model_weak: Some("claude-haiku-4-5".to_string()),
            ..Config::default()
        };
        let primary_config = config.to_model_config_for_role(ModelRole::Primary);
        assert_eq!(primary_config.model_name, "claude-sonnet-4-6");

        let weak_config = config.to_model_config_for_role(ModelRole::Weak);
        assert_eq!(weak_config.model_name, "claude-haiku-4-5");
    }

    #[test]
    fn test_fallback_models_default_empty() {
        let config = Config::default();
        assert!(config.fallback_models.is_empty());
    }

    #[test]
    fn test_config_deserialization_with_model_roles() {
        let yaml = r#"
model: claude-sonnet-4-6
model_weak: claude-haiku-4-5
model_reasoning: claude-opus-4-6
model_embedding: text-embedding-3-small
fallback_models:
  - gpt-4o
  - claude-sonnet-4-6
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model, "claude-sonnet-4-6");
        assert_eq!(config.model_weak, Some("claude-haiku-4-5".to_string()));
        assert_eq!(config.model_reasoning, Some("claude-opus-4-6".to_string()));
        assert_eq!(
            config.model_embedding,
            Some("text-embedding-3-small".to_string())
        );
        assert_eq!(config.fallback_models.len(), 2);
    }

    #[test]
    fn test_config_deserialization_without_model_roles() {
        // Existing configs without new fields should still work
        let yaml = r#"
model: claude-sonnet-4-6
temperature: 0.3
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model, "claude-sonnet-4-6");
        assert!(config.model_weak.is_none());
        assert!(config.model_reasoning.is_none());
        assert!(config.model_embedding.is_none());
        assert!(config.fallback_models.is_empty());
    }
}
