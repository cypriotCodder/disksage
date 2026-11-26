//! Scan a folder from the terminal, without the UI:
//!   cargo run --release --example scan -- C:\
use std::sync::Arc;

use disksage_core::candidates::{build_candidates, CandidateOptions};
use disksage_core::safety::SafetyGuard;
use disksage_core::scan::{scan, Progress, ScanOptions};
use disksage_core::util::human_size;

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let res = scan(root.as_ref(), &ScanOptions::default(), Arc::new(Progress::default())).expect("scan failed");
    println!(
        "{}: {} in {} files, {} folders, {} unreadable, {} ms",
        res.root,
        human_size(res.total_bytes),
        res.file_count,
        res.dir_count,
        res.errors,
        res.elapsed_ms
    );
    let cands = build_candidates(&res, &SafetyGuard::from_env(), &CandidateOptions::default());
    println!("\nTop candidates:");
    for c in cands.iter().take(25) {
        let v = c.effective_verdict().map(|v| format!("{v:?}")).unwrap_or_else(|| "Unknown".into());
        println!("{:>10}  {:<8} {:<26} {}", human_size(c.size), v, c.category, c.path);
    }
}
