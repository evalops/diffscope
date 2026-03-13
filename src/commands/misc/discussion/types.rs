use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct DiscussionTurn {
    pub(super) role: String,
    pub(super) message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct DiscussionThread {
    pub(super) comment_id: String,
    pub(super) turns: Vec<DiscussionTurn>,
}
