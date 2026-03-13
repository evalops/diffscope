#[path = "pattern/describe.rs"]
mod describe;
#[path = "pattern/matching.rs"]
mod matching;
#[path = "pattern/summary.rs"]
mod summary;

pub(super) use summary::summarize_for_eval;
