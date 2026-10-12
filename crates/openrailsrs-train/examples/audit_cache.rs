//! Read-only cold/warm dependency-cache benchmark, using the actual auditor.
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("pass a CONSISTS directory")?);
    let mut paths = std::fs::read_dir(&root)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("con")))
        .collect::<Vec<_>>();
    paths.sort();
    let mut auditor = openrailsrs_train::ConsistAuditor::new(vec![]);
    for round in ["cold", "warm"] {
        let before = auditor.work.clone();
        let clock = Instant::now();
        auditor.begin_batch();
        let ready = paths
            .iter()
            .map(|p| auditor.inspect(p))
            .filter(|a| a.player_ready())
            .count();
        auditor.end_batch();
        println!(
            "{}",
            serde_json::json!({"round":round,"formations":paths.len(),"player_ready":ready,"elapsed_ms":clock.elapsed().as_secs_f64()*1000.,"parsed_stocks":auditor.work.parsed_stocks-before.parsed_stocks,"reused_stocks":auditor.work.reused_stocks-before.reused_stocks,"parsed_shapes":auditor.work.parsed_shapes-before.parsed_shapes,"reused_shapes":auditor.work.reused_shapes-before.reused_shapes,"parsed_cabs":auditor.work.parsed_cabs-before.parsed_cabs,"reused_cabs":auditor.work.reused_cabs-before.reused_cabs})
        );
    }
    Ok(())
}
