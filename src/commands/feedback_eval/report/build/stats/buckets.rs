use std::collections::HashMap;

use super::super::super::super::FeedbackEvalBucket;

pub(in super::super) fn add_bucket_count(
    counts: &mut HashMap<String, (usize, usize)>,
    name: &str,
    accepted: bool,
) {
    let entry = counts.entry(name.to_string()).or_default();
    if accepted {
        entry.0 += 1;
    } else {
        entry.1 += 1;
    }
}

pub(in super::super) fn buckets_from_counts(
    counts: HashMap<String, (usize, usize)>,
) -> Vec<FeedbackEvalBucket> {
    let mut buckets = counts
        .into_iter()
        .map(|(name, (accepted, rejected))| build_bucket(name, accepted + rejected, accepted))
        .collect::<Vec<_>>();
    buckets.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| left.name.cmp(&right.name))
    });
    buckets
}

pub(in super::super) fn build_bucket(
    name: String,
    total: usize,
    accepted: usize,
) -> FeedbackEvalBucket {
    FeedbackEvalBucket {
        name,
        total,
        accepted,
        rejected: total.saturating_sub(accepted),
        acceptance_rate: ratio(accepted, total),
    }
}

pub(in super::super) fn ratio(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f32 / denominator as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_from_counts_orders_by_total_then_name() {
        let counts = HashMap::from([
            ("zeta".to_string(), (2, 1)),
            ("alpha".to_string(), (2, 1)),
            ("beta".to_string(), (1, 0)),
        ]);

        let buckets = buckets_from_counts(counts);

        assert_eq!(buckets[0].name, "alpha");
        assert_eq!(buckets[1].name, "zeta");
        assert_eq!(buckets[2].name, "beta");
    }
}
