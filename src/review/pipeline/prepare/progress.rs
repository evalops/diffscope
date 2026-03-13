use crate::core;

use super::super::contracts::{FileReviewJob, PreparedReviewJobs};
use super::super::session::ReviewSession;
use super::super::types::ProgressUpdate;

pub(super) struct PreparationProgress {
    all_comments: Vec<core::Comment>,
    files_completed: usize,
    files_skipped: usize,
}

impl PreparationProgress {
    pub(super) fn new() -> Self {
        Self {
            all_comments: Vec::new(),
            files_completed: 0,
            files_skipped: 0,
        }
    }

    pub(super) fn skip_file(&mut self) {
        self.files_skipped += 1;
    }

    pub(super) fn report_current_file(&self, session: &ReviewSession, diff: &core::UnifiedDiff) {
        self.emit_progress(session, diff);
    }

    pub(super) fn complete_with_comments(
        &mut self,
        session: &ReviewSession,
        diff: &core::UnifiedDiff,
        comments: Vec<core::Comment>,
    ) {
        self.all_comments.extend(comments);
        self.files_completed += 1;
        self.emit_progress(session, diff);
    }

    pub(super) fn into_prepared_review_jobs(self, jobs: Vec<FileReviewJob>) -> PreparedReviewJobs {
        PreparedReviewJobs {
            jobs,
            all_comments: self.all_comments,
            files_completed: self.files_completed,
            files_skipped: self.files_skipped,
        }
    }

    fn emit_progress(&self, session: &ReviewSession, diff: &core::UnifiedDiff) {
        if let Some(ref callback) = session.on_progress {
            callback(ProgressUpdate {
                current_file: diff.file_path.display().to_string(),
                files_total: session.files_total,
                files_completed: self.files_completed,
                files_skipped: self.files_skipped,
                comments_so_far: self.all_comments.clone(),
            });
        }
    }
}
