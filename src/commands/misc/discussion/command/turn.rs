use anyhow::Result;
use std::path::Path;

use crate::adapters;
use crate::core;

use super::super::prompt::answer_discussion_question;
use super::super::thread::save_discussion_thread;
use super::super::types::{DiscussionThread, DiscussionTurn};

pub(super) async fn run_discussion_turn(
    adapter: &dyn adapters::llm::LLMAdapter,
    selected: &core::Comment,
    thread: &mut DiscussionThread,
    question: String,
    thread_path: Option<&Path>,
) -> Result<()> {
    let answer = answer_discussion_question(adapter, selected, thread, &question).await?;

    println!("{}", answer.trim());

    thread.turns.push(DiscussionTurn {
        role: "user".to_string(),
        message: question,
    });
    thread.turns.push(DiscussionTurn {
        role: "assistant".to_string(),
        message: answer,
    });

    if let Some(path) = thread_path {
        save_discussion_thread(path, thread)?;
    }

    Ok(())
}
