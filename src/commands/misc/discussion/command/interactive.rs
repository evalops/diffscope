use anyhow::Result;
use std::path::Path;

use crate::adapters;
use crate::core;

use super::super::thread::read_follow_up_question;
use super::super::types::DiscussionThread;
use super::turn::run_discussion_turn;

pub(super) async fn run_interactive_discussion(
    adapter: &dyn adapters::llm::LLMAdapter,
    selected: &core::Comment,
    thread: &mut DiscussionThread,
    initial_question: Option<String>,
    thread_path: Option<&Path>,
) -> Result<()> {
    let mut next_question = initial_question;

    loop {
        let Some(question) = next_question.take().or(read_follow_up_question()?) else {
            break;
        };
        run_discussion_turn(adapter, selected, thread, question, thread_path).await?;
    }

    Ok(())
}
