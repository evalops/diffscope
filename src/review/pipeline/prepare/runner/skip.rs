use crate::core;

use super::super::super::services::PipelineServices;
use super::super::progress::PreparationProgress;

pub(super) fn skip_diff_if_needed(
    services: &PipelineServices,
    diff: &core::UnifiedDiff,
    progress: &mut PreparationProgress,
) -> bool {
    let skip_message = if services.config.should_exclude(&diff.file_path) {
        Some("Skipping excluded file")
    } else if diff.is_deleted {
        Some("Skipping deleted file")
    } else if diff.is_binary || diff.hunks.is_empty() {
        Some("Skipping non-text diff")
    } else {
        None
    };

    let Some(skip_message) = skip_message else {
        return false;
    };

    tracing::info!("{}: {}", skip_message, diff.file_path.display());
    progress.skip_file();
    true
}
