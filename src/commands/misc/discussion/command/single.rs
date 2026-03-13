use anyhow::Result;
use std::path::Path;

use crate::adapters;
use crate::core;

use super::super::types::DiscussionThread;
use super::turn::run_discussion_turn;

pub(super) async fn run_single_discussion(
    adapter: &dyn adapters::llm::LLMAdapter,
    selected: &core::Comment,
    thread: &mut DiscussionThread,
    question: String,
    thread_path: Option<&Path>,
) -> Result<()> {
    run_discussion_turn(adapter, selected, thread, question, thread_path).await
}
