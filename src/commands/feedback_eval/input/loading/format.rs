use serde_json::Value;

pub(super) enum FeedbackEvalInputFormat {
    ReviewMap,
    ReviewList,
    SemanticStore,
    CommentsJson,
}

pub(super) fn detect_feedback_eval_input_format(value: &Value) -> Option<FeedbackEvalInputFormat> {
    match value {
        Value::Object(map) => {
            if map.contains_key("examples") {
                Some(FeedbackEvalInputFormat::SemanticStore)
            } else if map.values().all(is_review_session_like) {
                Some(FeedbackEvalInputFormat::ReviewMap)
            } else {
                None
            }
        }
        Value::Array(items) => {
            let Some(first) = items.first() else {
                return Some(FeedbackEvalInputFormat::ReviewList);
            };
            if is_review_session_like(first) {
                Some(FeedbackEvalInputFormat::ReviewList)
            } else if is_comment_like(first) {
                Some(FeedbackEvalInputFormat::CommentsJson)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_review_session_like(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.contains_key("comments") || object.contains_key("event") || object.contains_key("id")
    })
}

fn is_comment_like(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.contains_key("file_path")
            || object.contains_key("content")
            || object.contains_key("feedback")
    })
}
