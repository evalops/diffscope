use super::super::EvalReport;

pub(in super::super) fn evaluation_failure_message(report: &EvalReport) -> Option<String> {
    if report.fixtures_failed == 0 && report.threshold_failures.is_empty() {
        return None;
    }

    let mut failure_parts = Vec::new();
    if report.fixtures_failed > 0 {
        failure_parts.push(format!(
            "{} fixture(s) did not meet expectations",
            report.fixtures_failed
        ));
    }
    if !report.threshold_failures.is_empty() {
        failure_parts.push(format!(
            "{} threshold check(s) failed",
            report.threshold_failures.len()
        ));
    }

    Some(format!("Evaluation failed: {}", failure_parts.join("; ")))
}
