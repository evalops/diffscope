use std::time::Duration;

use crate::core::offline::{LocalModel, ReadinessCheck};

pub(in super::super) fn print_recommended_model_summary(
    recommended: &LocalModel,
    estimated_ram_mb: usize,
    detected_context_window: Option<usize>,
    readiness: &ReadinessCheck,
) {
    println!("\nRecommended for code review: {}", recommended.name);
    println!("  Estimated RAM: ~{}MB", estimated_ram_mb);

    if let Some(ctx_size) = detected_context_window {
        println!(
            "  Context window: {} tokens (detected from model)",
            ctx_size
        );
    }

    if readiness.ready {
        println!("\nStatus: READY");
    } else {
        println!("\nStatus: NOT READY");
        for warning in &readiness.warnings {
            println!("  Warning: {}", warning);
        }
    }
}

pub(in super::super) fn print_inference_success(elapsed: Duration, tokens_per_sec: f64) {
    println!(
        "OK ({:.1}s, ~{:.0} tok/s)",
        elapsed.as_secs_f64(),
        tokens_per_sec
    );
    if tokens_per_sec < 2.0 {
        println!("  Warning: Very slow inference. Consider a smaller/quantized model.");
    }
}

pub(in super::super) fn print_inference_failure(error: &impl std::fmt::Display) {
    println!("FAILED");
    println!("  Error: {}", error);
    println!("  The model may still be loading. Try again in a moment.");
}

pub(in super::super) fn print_usage(base_url: &str, model_flag: &str) {
    println!("\nUsage:");
    println!(
        "  git diff | diffscope review --base-url {} --model {}",
        base_url, model_flag
    );
}
