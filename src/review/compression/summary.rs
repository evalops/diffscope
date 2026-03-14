use crate::core::diff_parser::UnifiedDiff;

/// Build a human-readable summary of skipped files.
pub fn build_skipped_summary(diffs: &[UnifiedDiff], skipped_indices: &[usize]) -> String {
    if skipped_indices.is_empty() {
        return String::new();
    }

    let mut deleted = Vec::new();
    let mut modified = Vec::new();

    for &idx in skipped_indices {
        if idx < diffs.len() {
            let diff = &diffs[idx];
            let path = diff.file_path.display().to_string();
            if diff.is_deleted {
                deleted.push(path);
            } else {
                modified.push(path);
            }
        }
    }

    let mut summary = String::new();
    if !deleted.is_empty() {
        summary.push_str("Deleted files (not reviewed):\n");
        for file in &deleted {
            summary.push_str(&format!("  - {file}\n"));
        }
    }
    if !modified.is_empty() {
        if !summary.is_empty() {
            summary.push('\n');
        }
        summary.push_str("Additional modified files (not reviewed due to context budget):\n");
        for file in &modified {
            summary.push_str(&format!("  - {file}\n"));
        }
    }
    summary
}
