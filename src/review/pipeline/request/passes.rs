use crate::config;
use crate::core;

pub(in super::super) fn specialized_passes(
    config: &config::Config,
) -> Vec<core::SpecializedPassKind> {
    if !config.multi_pass_specialized {
        return Vec::new();
    }

    let mut passes = vec![
        core::SpecializedPassKind::Security,
        core::SpecializedPassKind::Correctness,
    ];
    if config.strictness >= 2 {
        passes.push(core::SpecializedPassKind::Style);
    }
    passes
}
