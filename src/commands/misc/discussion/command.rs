use anyhow::Result;
use std::path::PathBuf;

#[path = "command/interactive.rs"]
mod interactive;
#[path = "command/single.rs"]
mod single;
#[path = "command/turn.rs"]
mod turn;

use crate::adapters;
use crate::config;

use super::candidates::{
    has_user_discussion_turns, print_discussion_candidates, suggest_discussion_candidates,
};
use super::selection::{load_discussion_comments, select_discussion_comment};
use super::thread::load_discussion_thread;
use interactive::run_interactive_discussion;
use single::run_single_discussion;

pub struct DiscussCommandRequest {
    pub review_path: PathBuf,
    pub comment_id: Option<String>,
    pub comment_index: Option<usize>,
    pub question: Option<String>,
    pub thread_path: Option<PathBuf>,
    pub interactive: bool,
    pub suggest_candidates: bool,
    pub candidate_output_json: bool,
}

pub async fn discuss_command(config: config::Config, request: DiscussCommandRequest) -> Result<()> {
    let comments = load_discussion_comments(&request.review_path).await?;
    let selected = select_discussion_comment(&comments, request.comment_id, request.comment_index)?;
    let mut thread = load_discussion_thread(request.thread_path.as_deref(), &selected.id);

    let model_config = config.to_model_config();
    let adapter = adapters::llm::create_adapter(&model_config)?;

    if request.question.is_none() && !request.interactive && !request.suggest_candidates {
        anyhow::bail!("Provide --question, use --interactive, or pass --suggest-candidates");
    }

    if request.interactive {
        run_interactive_discussion(
            adapter.as_ref(),
            &selected,
            &mut thread,
            request.question,
            request.thread_path.as_deref(),
        )
        .await?;
    } else if let Some(question) = request.question {
        run_single_discussion(
            adapter.as_ref(),
            &selected,
            &mut thread,
            question,
            request.thread_path.as_deref(),
        )
        .await?;
    }

    if request.suggest_candidates {
        if !has_user_discussion_turns(&thread) {
            anyhow::bail!("Generate at least one follow-up turn before using --suggest-candidates");
        }

        let suggestions =
            suggest_discussion_candidates(adapter.as_ref(), &selected, &thread).await?;
        print_discussion_candidates(&suggestions, request.candidate_output_json)?;
    }

    Ok(())
}
