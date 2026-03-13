use super::{Category, Comment, Severity};

pub(super) fn deduplicate_comments(comments: &mut Vec<Comment>) {
    comments.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
            .then(a.content.cmp(&b.content))
            .then(severity_rank(&a.severity).cmp(&severity_rank(&b.severity)))
    });
    comments.dedup_by(|a, b| {
        a.file_path == b.file_path && a.line_number == b.line_number && a.content == b.content
    });
}

pub(super) fn sort_by_priority(comments: &mut [Comment]) {
    comments.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then_with(|| category_rank(&a.category).cmp(&category_rank(&b.category)))
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| a.line_number.cmp(&b.line_number))
    });
}

fn severity_rank(severity: &Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Suggestion => 3,
    }
}

fn category_rank(category: &Category) -> u8 {
    match category {
        Category::Security => 0,
        Category::Bug => 1,
        Category::Performance => 2,
        Category::BestPractice => 3,
        Category::Style => 4,
        Category::Documentation => 5,
        Category::Maintainability => 6,
        Category::Testing => 7,
        Category::Architecture => 8,
    }
}
