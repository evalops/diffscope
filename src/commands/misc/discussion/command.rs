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

use super::selection::{load_discussion_comments, select_discussion_comment};
use super::thread::load_discussion_thread;
use interactive::run_interactive_discussion;
use single::run_single_discussion;

pub async fn discuss_command(
    config: config::Config,
    review_path: PathBuf,
    comment_id: Option<String>,
    comment_index: Option<usize>,
    question: Option<String>,
    thread_path: Option<PathBuf>,
    interactive: bool,
) -> Result<()> {
    let comments = load_discussion_comments(&review_path).await?;
    let selected = select_discussion_comment(&comments, comment_id, comment_index)?;
    let mut thread = load_discussion_thread(thread_path.as_deref(), &selected.id);

    let model_config = config.to_model_config();
    let adapter = adapters::llm::create_adapter(&model_config)?;

    if question.is_none() && !interactive {
        anyhow::bail!("Provide --question or use --interactive");
    }

    if interactive {
        run_interactive_discussion(
            adapter.as_ref(),
            &selected,
            &mut thread,
            question,
            thread_path.as_deref(),
        )
        .await?;
    } else if let Some(question) = question {
        run_single_discussion(
            adapter.as_ref(),
            &selected,
            &mut thread,
            question,
            thread_path.as_deref(),
        )
        .await?;
    }

    Ok(())
}
