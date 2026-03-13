use crate::core;

pub(super) fn find_dominated_comment_index(
    deduped: &[core::Comment],
    comment: &core::Comment,
) -> Option<usize> {
    deduped.iter().position(|existing| {
        existing.file_path == comment.file_path
            && existing.line_number == comment.line_number
            && core::multi_pass::content_similarity(&existing.content, &comment.content) > 0.6
    })
}
