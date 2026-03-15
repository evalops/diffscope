use super::*;

// === GitHub API helpers (use shared HTTP client for connection pooling) ===

pub(crate) fn log_rate_limit(resp: &reqwest::Response) {
    if let Some(remaining) = resp.headers().get("x-ratelimit-remaining") {
        if let Ok(remaining_str) = remaining.to_str() {
            if let Ok(n) = remaining_str.parse::<u32>() {
                if n < 10 {
                    warn!(remaining = n, "GitHub API rate limit low");
                }
            }
        }
    }
}

pub(crate) async fn github_api_get(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<reqwest::Response, String> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {e}"))?;

    log_rate_limit(&resp);

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        // Check for rate limit
        if let Some(remaining) = resp.headers().get("x-ratelimit-remaining") {
            if remaining.to_str().unwrap_or("1") == "0" {
                return Err("GitHub API rate limit exceeded. Wait and retry.".to_string());
            }
        }
    }

    Ok(resp)
}

pub(crate) async fn github_api_post(
    client: &reqwest::Client,
    token: &str,
    url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, String> {
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {e}"))?;

    log_rate_limit(&resp);
    Ok(resp)
}

/// GET a GitHub API URL, returning the raw diff text (used for PR diffs).
pub(crate) async fn github_api_get_diff(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<String, String> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github.v3.diff")
        .send()
        .await
        .map_err(|e| format!("GitHub API error: {e}"))?;

    log_rate_limit(&resp);

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API returned {status}: {body}"));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read diff response: {e}"))?;

    // Enforce diff size limit
    if text.len() > MAX_DIFF_SIZE {
        return Err(format!(
            "PR diff is {} MB, exceeds limit of {} MB",
            text.len() / (1024 * 1024),
            MAX_DIFF_SIZE / (1024 * 1024),
        ));
    }

    Ok(text)
}

pub(crate) async fn fetch_github_pr_head_sha(
    client: &reqwest::Client,
    token: &str,
    repo: &str,
    pr_number: u32,
) -> Result<String, String> {
    let pr_url = format!("https://api.github.com/repos/{}/pulls/{}", repo, pr_number);
    let pr_resp = github_api_get(client, token, &pr_url).await?;
    if !pr_resp.status().is_success() {
        let status = pr_resp.status();
        let body = pr_resp.text().await.unwrap_or_default();
        return Err(format!("Failed to get PR info {}: {}", status, body));
    }
    let pr_data: serde_json::Value = pr_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse PR response: {}", e))?;
    pr_data
        .get("head")
        .and_then(|head| head.get("sha"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "No head SHA in PR response".to_string())
}

// === GitHub integration types and handlers ===

#[derive(Serialize)]
pub(crate) struct GhStatusResponse {
    pub authenticated: bool,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub scopes: Vec<String>,
}

pub(crate) async fn get_gh_status(State(state): State<Arc<AppState>>) -> Json<GhStatusResponse> {
    let config = state.config.read().await;
    let token = match config.github.token.as_deref() {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                avatar_url: None,
                scopes: Vec::new(),
            });
        }
    };
    drop(config);

    let resp = match github_api_get(&state.http_client, &token, "https://api.github.com/user").await
    {
        Ok(r) => r,
        Err(_) => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                avatar_url: None,
                scopes: Vec::new(),
            });
        }
    };

    // Extract scopes from response header
    let scopes: Vec<String> = resp
        .headers()
        .get("x-oauth-scopes")
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(|scope| scope.trim().to_string())
                .filter(|scope| !scope.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if !resp.status().is_success() {
        return Json(GhStatusResponse {
            authenticated: false,
            username: None,
            avatar_url: None,
            scopes: Vec::new(),
        });
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(_) => {
            return Json(GhStatusResponse {
                authenticated: false,
                username: None,
                avatar_url: None,
                scopes: Vec::new(),
            });
        }
    };

    let username = body
        .get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let avatar_url = body
        .get("avatar_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Json(GhStatusResponse {
        authenticated: true,
        username,
        avatar_url,
        scopes,
    })
}

#[derive(Deserialize)]
pub(crate) struct GhReposParams {
    pub page: Option<usize>,
    pub per_page: Option<usize>,
    pub search: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct GhRepo {
    pub full_name: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub updated_at: String,
    pub open_prs: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_blockers: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking_prs: Option<usize>,
    pub default_branch: String,
    pub stargazers_count: u32,
    pub private: bool,
}

pub(crate) async fn get_gh_repos(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GhReposParams>,
) -> Result<Json<Vec<GhRepo>>, (StatusCode, String)> {
    let config = state.config.read().await;
    let token = config
        .github
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub token not configured. Set github_token in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let page = params.page.unwrap_or(1).clamp(1, 10_000);

    if let Some(ref search) = params.search {
        // First, get the authenticated user's login for the search query
        let username = get_github_username(&state.http_client, &token)
            .await
            .unwrap_or_default();
        let user_qualifier = if username.is_empty() {
            String::new()
        } else {
            format!("+user:{}", urlencoded(&username))
        };
        let url = format!(
            "https://api.github.com/search/repositories?q={}{}&per_page={}",
            urlencoded(search),
            user_qualifier,
            per_page,
        );

        let resp = github_api_get(&state.http_client, &token, &url)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("GitHub API returned {status}: {body}"),
            ));
        }

        let body: serde_json::Value = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse response: {e}"),
            )
        })?;

        let items = body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let inventory = load_review_inventory(&state).await;
        let blocker_rollups = build_repo_blocker_rollups(&inventory);

        let repos: Vec<GhRepo> = items
            .into_iter()
            .map(|item| GhRepo {
                full_name: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                language: item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                updated_at: item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                open_prs: 0,
                open_blockers: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .and_then(|repo| blocker_rollups.get(repo))
                    .map(|rollup| rollup.open_blockers),
                blocking_prs: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .and_then(|repo| blocker_rollups.get(repo))
                    .map(|rollup| rollup.blocking_prs),
                default_branch: item
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string(),
                stargazers_count: item
                    .get("stargazers_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                private: item
                    .get("private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
            .collect();

        Ok(Json(repos))
    } else {
        let url = format!(
            "https://api.github.com/user/repos?sort=updated&per_page={per_page}&page={page}",
        );

        let resp = github_api_get(&state.http_client, &token, &url)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err((
                StatusCode::BAD_GATEWAY,
                format!("GitHub API returned {status}: {body}"),
            ));
        }

        let items: Vec<serde_json::Value> = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to parse response: {e}"),
            )
        })?;

        let inventory = load_review_inventory(&state).await;
        let blocker_rollups = build_repo_blocker_rollups(&inventory);

        let repos: Vec<GhRepo> = items
            .into_iter()
            .map(|item| GhRepo {
                full_name: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                language: item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                updated_at: item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                open_prs: 0,
                open_blockers: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .and_then(|repo| blocker_rollups.get(repo))
                    .map(|rollup| rollup.open_blockers),
                blocking_prs: item
                    .get("full_name")
                    .and_then(|v| v.as_str())
                    .and_then(|repo| blocker_rollups.get(repo))
                    .map(|rollup| rollup.blocking_prs),
                default_branch: item
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string(),
                stargazers_count: item
                    .get("stargazers_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                private: item
                    .get("private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
            .collect();

        Ok(Json(repos))
    }
}

pub(crate) fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from(HEX_CHARS[(byte >> 4) as usize]));
                out.push(char::from(HEX_CHARS[(byte & 0x0F) as usize]));
            }
        }
    }
    out
}

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

/// Fetch the authenticated user's login from GitHub API.
pub(crate) async fn get_github_username(
    client: &reqwest::Client,
    token: &str,
) -> Result<String, String> {
    let resp = github_api_get(client, token, "https://api.github.com/user").await?;
    if !resp.status().is_success() {
        return Err("Failed to get user info".to_string());
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse user response: {e}"))?;
    body.get("login")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No login field in user response".to_string())
}

// === GitHub PRs ===

#[derive(Deserialize)]
pub(crate) struct GhPrsParams {
    pub repo: String,
    pub state: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PrReadinessParams {
    pub repo: String,
    pub pr_number: u32,
}

#[derive(Deserialize)]
pub(crate) struct PrCommentSearchParams {
    pub repo: String,
    pub pr_number: u32,
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PrFindingsParams {
    pub repo: String,
    pub pr_number: u32,
    pub group_by: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PrFixHandoffParams {
    pub repo: String,
    pub pr_number: u32,
    pub include_resolved: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommentSearchFilter {
    All,
    Unresolved,
    Resolved,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FindingsGroupBy {
    Severity,
    File,
    Lifecycle,
}

impl FindingsGroupBy {
    pub(crate) fn from_api_value(value: Option<&str>) -> Option<Self> {
        match value.map(|value| value.trim().to_ascii_lowercase()) {
            None => Some(Self::Severity),
            Some(value) if value.is_empty() || value == "severity" => Some(Self::Severity),
            Some(value) if value == "file" => Some(Self::File),
            Some(value) if value == "lifecycle" || value == "status" => Some(Self::Lifecycle),
            _ => None,
        }
    }

    pub(crate) fn as_api_str(self) -> &'static str {
        match self {
            Self::Severity => "severity",
            Self::File => "file",
            Self::Lifecycle => "lifecycle",
        }
    }
}

impl CommentSearchFilter {
    pub(crate) fn from_api_value(value: Option<&str>) -> Option<Self> {
        match value.map(|value| value.trim().to_ascii_lowercase()) {
            None => Some(Self::All),
            Some(value) if value.is_empty() || value == "all" => Some(Self::All),
            Some(value) if value == "open" || value == "unresolved" => Some(Self::Unresolved),
            Some(value) if value == "resolved" => Some(Self::Resolved),
            Some(value) if value == "dismissed" => Some(Self::Dismissed),
            _ => None,
        }
    }

    pub(crate) fn as_api_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Unresolved => "unresolved",
            Self::Resolved => "resolved",
            Self::Dismissed => "dismissed",
        }
    }

    pub(crate) fn matches(self, status: CommentStatus) -> bool {
        match self {
            Self::All => true,
            Self::Unresolved => status == CommentStatus::Open,
            Self::Resolved => status == CommentStatus::Resolved,
            Self::Dismissed => status == CommentStatus::Dismissed,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct PrCommentSearchResponse {
    pub repo: String,
    pub pr_number: u32,
    pub diff_source: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review_status: Option<ReviewStatus>,
    #[serde(default)]
    pub total_comments: usize,
    #[serde(default)]
    pub comments: Vec<crate::core::Comment>,
}

#[derive(Serialize)]
pub(crate) struct PrFindingsGroup {
    pub value: String,
    pub count: usize,
    #[serde(default)]
    pub findings: Vec<crate::core::Comment>,
}

#[derive(Serialize)]
pub(crate) struct PrFindingsResponse {
    pub repo: String,
    pub pr_number: u32,
    pub diff_source: String,
    pub group_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review_status: Option<ReviewStatus>,
    #[serde(default)]
    pub total_findings: usize,
    #[serde(default)]
    pub groups: Vec<PrFindingsGroup>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FixAgentEvidence {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FixAgentFinding {
    pub comment_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    pub file_path: String,
    pub line_number: usize,
    pub severity: crate::core::comment::Severity,
    pub category: crate::core::comment::Category,
    pub lifecycle_status: CommentStatus,
    pub fix_effort: crate::core::comment::FixEffort,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub evidence: FixAgentEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_diff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PrFixHandoffResponse {
    pub contract_version: u32,
    pub repo: String,
    pub pr_number: u32,
    pub diff_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_review_status: Option<ReviewStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_readiness: Option<MergeReadiness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_blockers: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readiness_reasons: Vec<String>,
    pub total_findings: usize,
    pub findings_included: usize,
    #[serde(default)]
    pub findings: Vec<FixAgentFinding>,
}

pub(crate) fn filter_comments_by_search_filter(
    comments: &[crate::core::Comment],
    filter: CommentSearchFilter,
) -> Vec<crate::core::Comment> {
    comments
        .iter()
        .filter(|comment| filter.matches(comment.status))
        .cloned()
        .collect()
}

pub(crate) fn group_pr_findings(
    findings: &[crate::core::Comment],
    group_by: FindingsGroupBy,
) -> Vec<PrFindingsGroup> {
    let mut grouped: BTreeMap<String, Vec<crate::core::Comment>> = BTreeMap::new();

    for finding in findings {
        let key = match group_by {
            FindingsGroupBy::Severity => match finding.severity {
                crate::core::comment::Severity::Error => "Error".to_string(),
                crate::core::comment::Severity::Warning => "Warning".to_string(),
                crate::core::comment::Severity::Info => "Info".to_string(),
                crate::core::comment::Severity::Suggestion => "Suggestion".to_string(),
            },
            FindingsGroupBy::File => finding.file_path.display().to_string(),
            FindingsGroupBy::Lifecycle => match finding.status {
                CommentStatus::Open => "Open".to_string(),
                CommentStatus::Resolved => "Resolved".to_string(),
                CommentStatus::Dismissed => "Dismissed".to_string(),
            },
        };

        grouped.entry(key).or_default().push(finding.clone());
    }

    let ordered_keys: Vec<String> = match group_by {
        FindingsGroupBy::Severity => ["Error", "Warning", "Info", "Suggestion"]
            .into_iter()
            .filter(|key| grouped.contains_key(*key))
            .map(str::to_string)
            .collect(),
        FindingsGroupBy::Lifecycle => ["Open", "Resolved", "Dismissed"]
            .into_iter()
            .filter(|key| grouped.contains_key(*key))
            .map(str::to_string)
            .collect(),
        FindingsGroupBy::File => grouped.keys().cloned().collect(),
    };

    ordered_keys
        .into_iter()
        .map(|value| {
            let findings = grouped.remove(&value).unwrap_or_default();
            PrFindingsGroup {
                count: findings.len(),
                value,
                findings,
            }
        })
        .collect()
}

fn should_include_fix_handoff_finding(
    comment: &crate::core::Comment,
    include_resolved: bool,
) -> bool {
    include_resolved || comment.status == CommentStatus::Open
}

fn fix_handoff_severity_rank(severity: &crate::core::comment::Severity) -> usize {
    match severity {
        crate::core::comment::Severity::Error => 0,
        crate::core::comment::Severity::Warning => 1,
        crate::core::comment::Severity::Info => 2,
        crate::core::comment::Severity::Suggestion => 3,
    }
}

pub(crate) fn build_fix_agent_findings(
    comments: &[crate::core::Comment],
    include_resolved: bool,
) -> Vec<FixAgentFinding> {
    let mut findings = comments
        .iter()
        .filter(|comment| should_include_fix_handoff_finding(comment, include_resolved))
        .map(|comment| FixAgentFinding {
            comment_id: comment.id.clone(),
            rule_id: comment.rule_id.clone(),
            file_path: comment.file_path.display().to_string(),
            line_number: comment.line_number,
            severity: comment.severity.clone(),
            category: comment.category.clone(),
            lifecycle_status: comment.status,
            fix_effort: comment.fix_effort.clone(),
            confidence: comment.confidence,
            tags: comment.tags.clone(),
            evidence: FixAgentEvidence {
                content: comment.content.clone(),
                suggestion: comment.suggestion.clone(),
                explanation: comment
                    .code_suggestion
                    .as_ref()
                    .map(|suggestion| suggestion.explanation.clone()),
                original_code: comment
                    .code_suggestion
                    .as_ref()
                    .map(|suggestion| suggestion.original_code.clone()),
            },
            suggested_diff: comment
                .code_suggestion
                .as_ref()
                .map(|suggestion| suggestion.diff.clone()),
            suggested_code: comment
                .code_suggestion
                .as_ref()
                .map(|suggestion| suggestion.suggested_code.clone()),
        })
        .collect::<Vec<_>>();

    findings.sort_by(|left, right| {
        fix_handoff_severity_rank(&left.severity)
            .cmp(&fix_handoff_severity_rank(&right.severity))
            .then_with(|| left.file_path.cmp(&right.file_path))
            .then_with(|| left.line_number.cmp(&right.line_number))
            .then_with(|| left.comment_id.cmp(&right.comment_id))
    });

    findings
}

pub(crate) fn build_pr_fix_handoff_response(
    repo: &str,
    pr_number: u32,
    latest_review: Option<&ReviewSession>,
    include_resolved: bool,
) -> PrFixHandoffResponse {
    let diff_source = pr_diff_source(repo, pr_number);
    let total_findings = latest_review
        .as_ref()
        .map(|review| review.comments.len())
        .unwrap_or_default();
    let findings = latest_review
        .as_ref()
        .map(|review| build_fix_agent_findings(&review.comments, include_resolved))
        .unwrap_or_default();
    let summary = latest_review
        .as_ref()
        .and_then(|review| review.summary.as_ref());

    PrFixHandoffResponse {
        contract_version: 1,
        repo: repo.to_string(),
        pr_number,
        diff_source,
        latest_review_id: latest_review.map(|review| review.id.clone()),
        latest_review_status: latest_review.map(|review| review.status.clone()),
        merge_readiness: summary.map(|summary| summary.merge_readiness),
        open_blockers: summary.map(|summary| summary.open_blockers),
        readiness_reasons: summary
            .map(|summary| summary.readiness_reasons.clone())
            .unwrap_or_default(),
        total_findings,
        findings_included: findings.len(),
        findings,
    }
}

#[derive(Serialize)]
pub(crate) struct GhPullRequest {
    pub number: u32,
    pub title: String,
    pub author: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub head_branch: String,
    pub base_branch: String,
    pub labels: Vec<String>,
    pub draft: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_blockers: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_readiness: Option<MergeReadiness>,
}

/// Regex for validating repo names: owner/repo
pub(crate) fn is_valid_repo_name(repo: &str) -> bool {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"^[a-zA-Z0-9._-]+/[a-zA-Z0-9._-]+$").unwrap()
    });
    RE.is_match(repo)
}

pub(crate) async fn get_gh_prs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GhPrsParams>,
) -> Result<Json<Vec<GhPullRequest>>, (StatusCode, String)> {
    if !is_valid_repo_name(&params.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    let pr_state = params.state.as_deref().unwrap_or("open");
    if !matches!(pr_state, "open" | "closed" | "all" | "merged") {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid state. Must be open, closed, merged, or all.".to_string(),
        ));
    }

    let config = state.config.read().await;
    let token = config
        .github
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub token not configured. Set github_token in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    // GitHub API uses "open" or "closed" for state; "all" is also valid.
    // "merged" is not a valid state filter in the API; use "closed" and filter by merged_at.
    let api_state = match pr_state {
        "merged" => "closed",
        other => other,
    };

    let url = format!(
        "https://api.github.com/repos/{}/pulls?state={}&per_page=30&sort=updated&direction=desc",
        params.repo, api_state,
    );

    let resp = github_api_get(&state.http_client, &token, &url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("GitHub API returned {status}: {body}"),
        ));
    }

    let items: Vec<serde_json::Value> = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to parse response: {e}"),
        )
    })?;

    let inventory = load_review_inventory(&state).await;

    let prs: Vec<GhPullRequest> = items
        .into_iter()
        .filter(|item| {
            // If the user asked for "merged", only include PRs that have merged_at set
            if pr_state == "merged" {
                item.get("merged_at").map(|v| !v.is_null()).unwrap_or(false)
            } else {
                true
            }
        })
        .map(|item| {
            let labels = item
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| {
                            l.get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();

            let state_str = if item.get("merged_at").map(|v| !v.is_null()).unwrap_or(false) {
                "merged".to_string()
            } else {
                item.get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };

            let pr_number = item.get("number").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let current_head_sha = item
                .get("head")
                .and_then(|v| v.get("sha"))
                .and_then(|v| v.as_str());
            let readiness_snapshot =
                build_pr_readiness_snapshot(&inventory, &params.repo, pr_number, current_head_sha);
            let latest_summary = readiness_snapshot
                .latest_review
                .as_ref()
                .and_then(|review| review.summary.as_ref());

            GhPullRequest {
                number: pr_number,
                title: item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                author: item
                    .get("user")
                    .and_then(|v| v.get("login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                state: state_str,
                created_at: item
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                updated_at: item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                additions: 0,
                deletions: 0,
                changed_files: 0,
                head_branch: item
                    .get("head")
                    .and_then(|v| v.get("ref"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                base_branch: item
                    .get("base")
                    .and_then(|v| v.get("ref"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                labels,
                draft: item.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
                open_blockers: latest_summary.map(|summary| summary.open_blockers),
                merge_readiness: latest_summary.map(|summary| summary.merge_readiness),
            }
        })
        .collect();

    Ok(Json(prs))
}

pub(crate) async fn get_gh_pr_readiness(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrReadinessParams>,
) -> Result<Json<PrReadinessSnapshot>, (StatusCode, String)> {
    if !is_valid_repo_name(&params.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    if params.pr_number == 0 || params.pr_number > 999_999_999 {
        return Err((StatusCode::BAD_REQUEST, "Invalid PR number.".to_string()));
    }

    let config_guard = state.config.read().await;
    let token = config_guard
        .github
        .token
        .clone()
        .filter(|value| !value.trim().is_empty());
    drop(config_guard);
    let current_head_sha = if let Some(ref token) = token {
        match fetch_github_pr_head_sha(&state.http_client, token, &params.repo, params.pr_number)
            .await
        {
            Ok(head_sha) => Some(head_sha),
            Err(err) => {
                warn!(
                    repo = %params.repo,
                    pr_number = params.pr_number,
                    "Failed to fetch current PR head SHA for readiness: {}",
                    err
                );
                None
            }
        }
    } else {
        None
    };

    Ok(Json(
        get_pr_readiness_snapshot(
            &state,
            &params.repo,
            params.pr_number,
            current_head_sha.as_deref(),
        )
        .await,
    ))
}

pub(crate) async fn get_gh_pr_comments(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrCommentSearchParams>,
) -> Result<Json<PrCommentSearchResponse>, (StatusCode, String)> {
    if !is_valid_repo_name(&params.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    if params.pr_number == 0 || params.pr_number > 999_999_999 {
        return Err((StatusCode::BAD_REQUEST, "Invalid PR number.".to_string()));
    }

    let filter =
        CommentSearchFilter::from_api_value(params.status.as_deref()).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "Invalid status. Must be all, unresolved, open, resolved, or dismissed."
                    .to_string(),
            )
        })?;

    let inventory = load_review_inventory(&state).await;
    let latest_review = latest_pr_review_session(&inventory, &params.repo, params.pr_number);
    let comments = latest_review
        .as_ref()
        .map(|review| filter_comments_by_search_filter(&review.comments, filter))
        .unwrap_or_default();

    Ok(Json(PrCommentSearchResponse {
        repo: params.repo.clone(),
        pr_number: params.pr_number,
        diff_source: pr_diff_source(&params.repo, params.pr_number),
        status: filter.as_api_str().to_string(),
        latest_review_id: latest_review.as_ref().map(|review| review.id.clone()),
        latest_review_status: latest_review.as_ref().map(|review| review.status.clone()),
        total_comments: comments.len(),
        comments,
    }))
}

pub(crate) async fn get_gh_pr_findings(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrFindingsParams>,
) -> Result<Json<PrFindingsResponse>, (StatusCode, String)> {
    if !is_valid_repo_name(&params.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    if params.pr_number == 0 || params.pr_number > 999_999_999 {
        return Err((StatusCode::BAD_REQUEST, "Invalid PR number.".to_string()));
    }

    let group_by =
        FindingsGroupBy::from_api_value(params.group_by.as_deref()).ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "Invalid group_by. Must be severity, file, or lifecycle.".to_string(),
            )
        })?;

    let inventory = load_review_inventory(&state).await;
    let latest_review = latest_pr_review_session(&inventory, &params.repo, params.pr_number);
    let findings = latest_review
        .as_ref()
        .map(|review| review.comments.clone())
        .unwrap_or_default();
    let groups = group_pr_findings(&findings, group_by);

    Ok(Json(PrFindingsResponse {
        repo: params.repo.clone(),
        pr_number: params.pr_number,
        diff_source: pr_diff_source(&params.repo, params.pr_number),
        group_by: group_by.as_api_str().to_string(),
        latest_review_id: latest_review.as_ref().map(|review| review.id.clone()),
        latest_review_status: latest_review.as_ref().map(|review| review.status.clone()),
        total_findings: findings.len(),
        groups,
    }))
}

pub(crate) async fn get_gh_pr_fix_handoff(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrFixHandoffParams>,
) -> Result<Json<PrFixHandoffResponse>, (StatusCode, String)> {
    if !is_valid_repo_name(&params.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    if params.pr_number == 0 || params.pr_number > 999_999_999 {
        return Err((StatusCode::BAD_REQUEST, "Invalid PR number.".to_string()));
    }

    let include_resolved = params.include_resolved.unwrap_or(false);
    let inventory = load_review_inventory(&state).await;
    let latest_review = latest_pr_review_session(&inventory, &params.repo, params.pr_number);

    Ok(Json(build_pr_fix_handoff_response(
        &params.repo,
        params.pr_number,
        latest_review.as_ref(),
        include_resolved,
    )))
}

// === GitHub PR Review ===

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct StartPrReviewRequest {
    pub repo: String,
    pub pr_number: u32,
    pub post_results: bool,
}

#[derive(Deserialize)]
pub(crate) struct RerunPrReviewRequest {
    pub review_id: String,
    pub post_results: Option<bool>,
}

pub(crate) fn resolve_rerun_post_results(
    session: &ReviewSession,
    post_results_override: Option<bool>,
) -> bool {
    post_results_override
        .or(session.github_post_results_requested)
        .or_else(|| session.event.as_ref().map(|event| event.github_posted))
        .unwrap_or(false)
}

pub(crate) fn build_rerun_pr_review_request(
    session: &ReviewSession,
    post_results_override: Option<bool>,
) -> Result<StartPrReviewRequest, (StatusCode, String)> {
    let Some((repo, pr_number)) = parse_pr_diff_source(&session.diff_source) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Review is not tied to a GitHub PR.".to_string(),
        ));
    };

    Ok(StartPrReviewRequest {
        repo,
        pr_number,
        post_results: resolve_rerun_post_results(session, post_results_override),
    })
}

pub(crate) async fn dispatch_pr_review(
    state: &Arc<AppState>,
    request: StartPrReviewRequest,
) -> Result<StartReviewResponse, (StatusCode, String)> {
    info!(repo = %request.repo, pr = request.pr_number, post_results = request.post_results, "Starting PR review");

    if !is_valid_repo_name(&request.repo) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid repo format. Expected 'owner/repo'.".to_string(),
        ));
    }

    if request.pr_number == 0 || request.pr_number > 999_999_999 {
        return Err((StatusCode::BAD_REQUEST, "Invalid PR number.".to_string()));
    }

    let config = state.config.read().await;
    let token = config
        .github
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "GitHub token not configured. Set github_token in config.".to_string(),
            )
        })?
        .to_string();
    drop(config);

    let diff_url = format!(
        "https://api.github.com/repos/{}/pulls/{}",
        request.repo, request.pr_number,
    );
    let diff_content = github_api_get_diff(&state.http_client, &token, &diff_url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    let head_sha =
        fetch_github_pr_head_sha(&state.http_client, &token, &request.repo, request.pr_number)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;

    let id = Uuid::new_v4().to_string();
    let diff_source = pr_diff_source(&request.repo, request.pr_number);

    let session = ReviewSession {
        id: id.clone(),
        status: ReviewStatus::Pending,
        diff_source: diff_source.clone(),
        github_head_sha: Some(head_sha.clone()),
        github_post_results_requested: Some(request.post_results),
        started_at: current_timestamp(),
        completed_at: None,
        comments: Vec::new(),
        summary: None,
        files_reviewed: 0,
        error: None,
        pr_summary_text: None,
        diff_content: Some(diff_content.clone()),
        event: None,
        progress: None,
    };

    state.reviews.write().await.insert(id.clone(), session);

    let state_clone = state.clone();
    let review_id = id.clone();
    let repo = request.repo.clone();
    let pr_number = request.pr_number;
    let pr_head_sha = head_sha.clone();
    let post_results = request.post_results;

    tokio::spawn(async move {
        run_pr_review_task(
            state_clone,
            review_id,
            diff_content,
            repo,
            pr_number,
            pr_head_sha,
            post_results,
        )
        .await;
    });

    Ok(StartReviewResponse {
        id,
        status: ReviewStatus::Pending,
    })
}

#[tracing::instrument(name = "api.start_pr_review", skip(state, request), fields(repo = %request.repo, pr_number = request.pr_number))]
pub(crate) async fn start_pr_review(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartPrReviewRequest>,
) -> Result<Json<StartReviewResponse>, (StatusCode, String)> {
    Ok(Json(dispatch_pr_review(&state, request).await?))
}

#[tracing::instrument(name = "api.rerun_pr_review", skip(state, request), fields(review_id = %request.review_id))]
pub(crate) async fn rerun_pr_review(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RerunPrReviewRequest>,
) -> Result<Json<StartReviewResponse>, (StatusCode, String)> {
    let review_id = request.review_id.trim();
    let session = load_review_session_for_update(&state, review_id)
        .await
        .map_err(|status| match status {
            StatusCode::NOT_FOUND => (
                StatusCode::NOT_FOUND,
                format!("Review '{}' not found.", review_id),
            ),
            _ => (status, "Failed to load review session.".to_string()),
        })?;

    let start_request = build_rerun_pr_review_request(&session, request.post_results)?;

    Ok(Json(dispatch_pr_review(&state, start_request).await?))
}

pub(crate) async fn run_pr_review_task(
    state: Arc<AppState>,
    review_id: String,
    diff_content: String,
    repo: String,
    pr_number: u32,
    _head_sha: String,
    post_results: bool,
) {
    let _permit = match state.review_semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            AppState::fail_review(
                &state,
                &review_id,
                "Review semaphore closed".to_string(),
                None,
            )
            .await;
            return;
        }
    };

    let task_start = std::time::Instant::now();
    let diff_source = format!("pr:{repo}#{pr_number}");
    AppState::mark_running(&state, &review_id).await;

    let config = state.config.read().await.clone();
    let repo_path = state.repo_path.clone();
    let github_token = config.github.token.clone();
    let model = config.model.clone();
    let provider = config.adapter.clone();
    let base_url = config.base_url.clone();
    let summary_config = if config.smart_review_summary {
        Some(config.clone())
    } else {
        None
    };

    let diff_bytes = diff_content.len();
    let diff_files_total = count_diff_files(&diff_content);

    if diff_content.trim().is_empty() {
        let event = ReviewEventBuilder::new(&review_id, "review.completed", &diff_source, &model)
            .provider(provider.as_deref())
            .base_url(base_url.as_deref())
            .duration_ms(task_start.elapsed().as_millis() as u64)
            .github(&repo, pr_number)
            .build();
        emit_wide_event(&event);
        AppState::complete_review(
            &state,
            &review_id,
            Vec::new(),
            CommentSynthesizer::generate_summary(&[]),
            0,
            event,
        )
        .await;
        AppState::save_reviews_async(&state);
        return;
    }

    let llm_start = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        crate::review::review_diff_content_raw(&diff_content, config, &repo_path),
    )
    .await;
    let llm_ms = llm_start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(review_result)) => {
            let comments = review_result.comments;
            let summary = CommentSynthesizer::apply_verification(
                CommentSynthesizer::generate_summary(&comments),
                crate::review::summarize_review_verification(
                    review_result.verification_report.as_ref(),
                    &review_result.warnings,
                ),
            );
            let files_reviewed = count_reviewed_files(&comments);

            let mut github_posted = false;
            if post_results && !comments.is_empty() {
                if let Some(ref token) = github_token {
                    github_posted = post_pr_review_comments(
                        &state.http_client,
                        token,
                        &repo,
                        pr_number,
                        &comments,
                        Some(&summary),
                    )
                    .await
                    .is_ok();
                }
            }

            let file_metric_events: Vec<FileMetricEvent> = review_result
                .file_metrics
                .iter()
                .map(|m| FileMetricEvent {
                    file_path: m.file_path.display().to_string(),
                    latency_ms: m.latency_ms,
                    prompt_tokens: m.prompt_tokens,
                    completion_tokens: m.completion_tokens,
                    total_tokens: m.total_tokens,
                    comment_count: m.comment_count,
                })
                .collect();
            let event =
                ReviewEventBuilder::new(&review_id, "review.completed", &diff_source, &model)
                    .provider(provider.as_deref())
                    .base_url(base_url.as_deref())
                    .duration_ms(task_start.elapsed().as_millis() as u64)
                    .llm_total_ms(llm_ms)
                    .diff_stats(
                        diff_bytes,
                        diff_files_total,
                        files_reviewed,
                        diff_files_total.saturating_sub(files_reviewed),
                    )
                    .comments(&comments, Some(&summary))
                    .tokens(
                        review_result.total_prompt_tokens,
                        review_result.total_completion_tokens,
                        review_result.total_tokens,
                    )
                    .file_metrics(file_metric_events)
                    .hotspot_details(
                        review_result
                            .hotspots
                            .iter()
                            .map(|h| HotspotDetail {
                                file_path: h.file_path.display().to_string(),
                                risk_score: h.risk_score,
                                reasons: h.reasons.clone(),
                            })
                            .collect(),
                    )
                    .convention_suppressed(review_result.convention_suppressed_count)
                    .comments_by_pass(review_result.comments_by_pass)
                    .agent_activity(review_result.agent_activity.as_ref())
                    .github(&repo, pr_number)
                    .github_posted(github_posted)
                    .build();
            emit_wide_event(&event);
            AppState::complete_review(&state, &review_id, comments, summary, files_reviewed, event)
                .await;

            // Generate AI-powered PR summary if enabled
            if let Some(ref cfg) = summary_config {
                generate_and_store_pr_summary(&state, &review_id, &diff_content, cfg).await;
            }
        }
        Ok(Err(e)) => {
            let err_msg = format!("Review failed: {e}");
            let event = ReviewEventBuilder::new(&review_id, "review.failed", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .llm_total_ms(llm_ms)
                .diff_stats(diff_bytes, diff_files_total, 0, 0)
                .github(&repo, pr_number)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
        }
        Err(_) => {
            let err_msg = "Review timed out after 5 minutes".to_string();
            let event = ReviewEventBuilder::new(&review_id, "review.timeout", &diff_source, &model)
                .provider(provider.as_deref())
                .base_url(base_url.as_deref())
                .duration_ms(task_start.elapsed().as_millis() as u64)
                .llm_total_ms(llm_ms)
                .diff_stats(diff_bytes, diff_files_total, 0, 0)
                .github(&repo, pr_number)
                .error(&err_msg)
                .build();
            emit_wide_event(&event);
            AppState::fail_review(&state, &review_id, err_msg, Some(event)).await;
        }
    }

    AppState::save_reviews_async(&state);
    AppState::prune_old_reviews(&state).await;

    // Persist to storage backend (PostgreSQL or JSON)
    {
        let reviews = state.reviews.read().await;
        if let Some(session) = reviews.get(&review_id) {
            if let Err(e) = state.storage.save_review(session).await {
                warn!(review_id = %review_id, "Failed to persist PR review to storage: {}", e);
            }
            if let Some(ref event) = session.event {
                if let Err(e) = state.storage.save_event(event).await {
                    warn!(review_id = %review_id, "Failed to persist PR event to storage: {}", e);
                }
            }
        }
    }
}

/// Generate an AI-powered PR summary and store it in the review session.
/// Called after a successful review when `smart_review_summary` is enabled.
///
/// GitIntegration contains a raw pointer and is not `Sync`, so git operations
/// are performed in a blocking task before the async LLM call.
pub(crate) async fn generate_and_store_pr_summary(
    state: &Arc<AppState>,
    review_id: &str,
    diff_content: &str,
    config: &crate::config::Config,
) {
    use crate::core::{DiffParser, PRSummaryGenerator, SummaryOptions};

    let diffs = match DiffParser::parse_unified_diff(diff_content) {
        Ok(d) => d,
        Err(e) => {
            warn!(review_id = %review_id, "PR summary skipped (diff parse error): {}", e);
            return;
        }
    };

    // Extract recent commits in a blocking task (GitIntegration is not Sync)
    let repo_path = state.repo_path.clone();
    let commits = match tokio::task::spawn_blocking(move || {
        let git = crate::core::GitIntegration::new(&repo_path)?;
        git.get_recent_commits(10)
    })
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            warn!(review_id = %review_id, "PR summary skipped (git error): {}", e);
            return;
        }
        Err(e) => {
            warn!(review_id = %review_id, "PR summary skipped (blocking task failed): {}", e);
            return;
        }
    };

    // Use Fast model for PR summary generation (lightweight task)
    let fast_config = config.to_model_config_for_role(crate::config::ModelRole::Fast);
    let adapter = match crate::adapters::llm::create_adapter(&fast_config) {
        Ok(a) => a,
        Err(e) => {
            warn!(review_id = %review_id, "PR summary skipped (adapter error): {}", e);
            return;
        }
    };

    let options = SummaryOptions {
        include_diagram: false,
    };

    match PRSummaryGenerator::generate_summary_with_commits(
        &diffs,
        &commits,
        adapter.as_ref(),
        options,
    )
    .await
    {
        Ok(summary) => {
            let markdown = summary.to_markdown();
            info!(review_id = %review_id, "PR summary generated ({} chars)", markdown.len());
            let mut reviews = state.reviews.write().await;
            if let Some(session) = reviews.get_mut(review_id) {
                session.pr_summary_text = Some(markdown);
            }
        }
        Err(e) => {
            warn!(review_id = %review_id, "PR summary generation failed: {}", e);
        }
    }
}

pub(crate) async fn post_pr_review_comments(
    client: &reqwest::Client,
    token: &str,
    repo: &str,
    pr_number: u32,
    comments: &[crate::core::Comment],
    summary: Option<&crate::core::comment::ReviewSummary>,
) -> Result<(), String> {
    // Fetch PR head SHA (required for inline comments)
    let pr_url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}",);
    let pr_resp = github_api_get(client, token, &pr_url).await?;
    if !pr_resp.status().is_success() {
        let status = pr_resp.status();
        let body = pr_resp.text().await.unwrap_or_default();
        return Err(format!("Failed to get PR info {status}: {body}"));
    }
    let pr_data: serde_json::Value = pr_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse PR response: {e}"))?;
    let commit_id = pr_data
        .get("head")
        .and_then(|h| h.get("sha"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "No head SHA in PR response".to_string())?
        .to_string();

    // Build inline review comments
    let mut inline_comments = Vec::new();
    for c in comments {
        let severity_icon = match c.severity {
            crate::core::comment::Severity::Error => ":rotating_light:",
            crate::core::comment::Severity::Warning => ":warning:",
            crate::core::comment::Severity::Info => ":information_source:",
            crate::core::comment::Severity::Suggestion => ":bulb:",
        };

        let mut comment_body = format!("{} **{}** | {}", severity_icon, c.severity, c.category);
        if c.confidence > 0.0 {
            comment_body.push_str(&format!(
                " | confidence: {}%",
                (c.confidence * 100.0) as u32
            ));
        }
        comment_body.push_str(&format!("\n\n{}", c.content));

        if let Some(ref suggestion) = c.suggestion {
            comment_body.push_str(&format!("\n\n> **Suggestion:** {suggestion}"));
        }
        // Calculate the line span for multi-line suggestions.
        // GitHub requires `start_line` + `line` when a suggestion covers multiple lines.
        let suggestion_line_span: Option<usize> = c
            .code_suggestion
            .as_ref()
            .map(|cs| cs.original_code.lines().count().max(1));

        if let Some(ref cs) = c.code_suggestion {
            if !cs.explanation.is_empty() {
                comment_body.push_str(&format!("\n\n**Suggested fix:** {}", cs.explanation));
            } else {
                comment_body.push_str("\n\n**Suggested fix:**");
            }
            comment_body.push_str(&format!("\n```suggestion\n{}\n```", cs.suggested_code));
        }

        // Normalize file path (strip leading / or a/ b/ prefixes)
        let path = c.file_path.display().to_string();
        let path = path.trim_start_matches('/');
        let path = if path.starts_with("a/") || path.starts_with("b/") {
            &path[2..]
        } else {
            path
        };

        let mut comment_json = serde_json::json!({
            "path": path,
            "line": c.line_number,
            "side": "RIGHT",
            "body": comment_body,
        });

        // For multi-line suggestions, set start_line so GitHub knows the full
        // range of lines being replaced by the suggestion block.
        if let Some(span) = suggestion_line_span {
            if span > 1 {
                let end_line = c.line_number + span - 1;
                comment_json["start_line"] = serde_json::json!(c.line_number);
                comment_json["line"] = serde_json::json!(end_line);
                comment_json["start_side"] = serde_json::json!("RIGHT");
            }
        }

        inline_comments.push(comment_json);
    }

    // Build summary body
    let mut review_body_text = String::from("## DiffScope Review\n\n");
    if let Some(s) = summary {
        review_body_text.push_str(&format!(
            "**Score:** {}/10 | **Findings:** {}\n\n",
            s.overall_score, s.total_comments
        ));
        if !s.recommendations.is_empty() {
            review_body_text.push_str("**Recommendations:**\n");
            for rec in &s.recommendations {
                review_body_text.push_str(&format!("- {rec}\n"));
            }
            review_body_text.push('\n');
        }
    } else {
        review_body_text.push_str(&format!(
            "Found **{}** issue{}.\n\n",
            comments.len(),
            if comments.len() == 1 { "" } else { "s" }
        ));
    }
    review_body_text
        .push_str("_Automated review by [DiffScope](https://github.com/evalops/diffscope)_");

    // Determine event type based on severity
    let has_errors = comments
        .iter()
        .any(|c| matches!(c.severity, crate::core::comment::Severity::Error));
    let event = if has_errors {
        "REQUEST_CHANGES"
    } else {
        "COMMENT"
    };

    let review_payload = serde_json::json!({
        "commit_id": commit_id,
        "body": review_body_text,
        "event": event,
        "comments": inline_comments,
    });

    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}/reviews",);

    let resp = github_api_post(client, token, &url, &review_payload).await?;

    if resp.status().is_success() {
        info!(repo = %repo, pr = pr_number, comments = comments.len(), "Posted inline review to GitHub");
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!("GitHub API returned {status}: {body}"))
    }
}
