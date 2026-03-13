use anyhow::Result;
use std::path::PathBuf;

use crate::adapters;
use crate::config;

use super::prompt::answer_discussion_question;
use super::selection::{load_discussion_comments, select_discussion_comment};
use super::thread::{load_discussion_thread, read_follow_up_question, save_discussion_thread};
use super::types::DiscussionTurn;

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

    let mut next_question = question;
    if next_question.is_none() && !interactive {
        anyhow::bail!("Provide --question or use --interactive");
    }

    loop {
        let current_question = if let Some(question) = next_question.take() {
            question
        } else if interactive {
            match read_follow_up_question()? {
                Some(question) => question,
                None => break,
            }
        } else {
            break;
        };

        let answer =
            answer_discussion_question(adapter.as_ref(), &selected, &thread, &current_question)
                .await?;

        println!("{}", answer.trim());

        thread.turns.push(DiscussionTurn {
            role: "user".to_string(),
            message: current_question,
        });
        thread.turns.push(DiscussionTurn {
            role: "assistant".to_string(),
            message: answer,
        });

        if let Some(path) = &thread_path {
            save_discussion_thread(path, &thread)?;
        }

        if !interactive {
            break;
        }
    }

    Ok(())
}
