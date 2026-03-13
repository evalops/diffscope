#[path = "examples/build.rs"]
mod build;
#[path = "examples/ranking.rs"]
mod ranking;

pub(super) use build::{build_showcase_candidates, build_vague_rejections};
