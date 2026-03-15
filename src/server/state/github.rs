use super::*;

impl AppState {
    /// Record the head SHA after a successful review for a PR.
    /// The key is formatted as "owner/repo#pr_number".
    pub async fn record_reviewed_sha(state: &Arc<AppState>, pr_key: &str, head_sha: &str) {
        let mut shas = state.last_reviewed_shas.write().await;
        shas.insert(pr_key.to_string(), head_sha.to_string());
    }

    /// Look up the last reviewed head SHA for a PR.
    /// Returns `None` if this PR has never been reviewed.
    pub async fn get_last_reviewed_sha(state: &Arc<AppState>, pr_key: &str) -> Option<String> {
        let shas = state.last_reviewed_shas.read().await;
        shas.get(pr_key).cloned()
    }

    /// Look up the in-memory verification reuse cache for a PR.
    pub async fn get_pr_verification_reuse_cache(
        state: &Arc<AppState>,
        pr_key: &str,
    ) -> crate::review::verification::VerificationReuseCache {
        let caches = state.pr_verification_reuse_caches.read().await;
        caches.get(pr_key).cloned().unwrap_or_default()
    }

    /// Replace the in-memory verification reuse cache for a PR.
    pub async fn store_pr_verification_reuse_cache(
        state: &Arc<AppState>,
        pr_key: &str,
        cache: crate::review::verification::VerificationReuseCache,
    ) {
        let mut caches = state.pr_verification_reuse_caches.write().await;
        if cache.is_empty() {
            caches.remove(pr_key);
        } else {
            caches.insert(pr_key.to_string(), cache);
        }
    }
}
