//! Picks what goes on the Cleanup screen.
//!
//! 1. Folders a rule recognises (topmost match only; we don't descend into it).
//! 2. Large files, with a rule verdict when one applies.
//! 3. Large unknown folders where the space is concentrated: these are what the
//!    AI is asked about.

use serde::{Deserialize, Serialize};

use crate::rules::{self, DirInfo};
use crate::safety::SafetyGuard;
use crate::scan::ScanResult;
use crate::util::{self, is_within};
use crate::Verdict;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Rule,
    LargeFile,
    LargeFolder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiNote {
    pub what: String,
    pub verdict: Verdict,
    pub reason: String,
    pub tip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: u32,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub file_count: u64,
    pub modified: i64,
    pub source: Source,
    pub category: String,
    /// Verdict from a rule (None = unknown, ask the AI).
    pub verdict: Option<Verdict>,
    pub reason: String,
    /// When set, reclaim the space this way instead of deleting.
    pub manual: Option<String>,
    /// Protected by the safety guard: can never be selected.
    pub protected: bool,
    pub ai: Option<AiNote>,
}

impl Candidate {
    /// What the UI should treat as the final verdict.
    pub fn effective_verdict(&self) -> Option<Verdict> {
        if self.protected {
            return Some(Verdict::Keep);
        }
        self.verdict.or(self.ai.as_ref().map(|a| a.verdict))
    }

    pub fn can_recycle(&self) -> bool {
        !self.protected && self.manual.is_none() && self.effective_verdict() != Some(Verdict::Keep)
    }
}

#[derive(Debug, Clone)]
pub struct CandidateOptions {
    pub min_rule_bytes: u64,
    pub min_large_file: u64,
    pub min_large_folder: u64,
    /// A folder is "concentrated" when no single child holds more than this share.
    pub concentration: f64,
    pub max_large_files: usize,
    pub max_large_folders: usize,
}

impl Default for CandidateOptions {
    fn default() -> Self {
        Self {
            min_rule_bytes: 10 * 1024 * 1024,
            min_large_file: 200 * 1024 * 1024,
            min_large_folder: 500 * 1024 * 1024,
            concentration: 0.6,
            max_large_files: 150,
            max_large_folders: 150,
        }
    }
}

pub fn build_candidates(scan: &ScanResult, guard: &SafetyGuard, opts: &CandidateOptions) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut claimed: Vec<String> = Vec::new(); // normalized folders already covered

    // 1) rule matches on folders, top-down
    let mut stack = vec![scan.root_index];
    while let Some(i) = stack.pop() {
        let d = &scan.dirs[i];
        if d.total_bytes < opts.min_rule_bytes {
            continue; // nothing below can be big enough either
        }
        let norm = util::normalize(&d.path);
        let info = DirInfo { norm: &norm, path: &d.path, depth_from_drive: util::depth_from_root(&norm) };
        if let Some(hit) = rules::classify_dir(&info) {
            out.push(Candidate {
                id: 0,
                path: d.path.clone(),
                is_dir: true,
                size: d.total_bytes,
                file_count: d.file_count,
                modified: d.latest_mtime,
                source: Source::Rule,
                category: hit.category.into(),
                verdict: Some(hit.verdict),
                reason: hit.reason.into(),
                manual: hit.manual.map(Into::into),
                protected: guard.is_protected(&d.path),
                ai: None,
            });
            claimed.push(norm);
            continue;
        }
        stack.extend(scan.children[i].iter().copied());
    }

    let under_claimed = |norm: &str, claimed: &[String]| claimed.iter().any(|c| is_within(norm, c));

    // 2) large files
    let mut large_count = 0;
    for f in &scan.large_files {
        let norm = util::normalize(&f.path);
        if under_claimed(&norm, &claimed) {
            continue;
        }
        let hit = rules::classify_file(&norm);
        if hit.is_none() && (f.size < opts.min_large_file || large_count >= opts.max_large_files) {
            continue;
        }
        if hit.is_some() && f.size < opts.min_rule_bytes {
            continue;
        }
        if hit.is_none() {
            large_count += 1;
        }
        out.push(Candidate {
            id: 0,
            path: f.path.clone(),
            is_dir: false,
            size: f.size,
            file_count: 1,
            modified: f.modified,
            source: if hit.is_some() { Source::Rule } else { Source::LargeFile },
            category: hit.as_ref().map(|h| h.category).unwrap_or("Large files").into(),
            verdict: hit.as_ref().map(|h| h.verdict),
            reason: hit.as_ref().map(|h| h.reason).unwrap_or("").into(),
            manual: hit.as_ref().and_then(|h| h.manual).map(Into::into),
            protected: guard.is_protected(&f.path),
            ai: None,
        });
    }

    // 3) large unknown folders where space is concentrated
    let mut folders = Vec::new();
    let mut stack = vec![scan.root_index];
    while let Some(i) = stack.pop() {
        let d = &scan.dirs[i];
        if d.total_bytes < opts.min_large_folder {
            continue;
        }
        let norm = util::normalize(&d.path);
        if under_claimed(&norm, &claimed) {
            continue;
        }
        let protected = guard.is_protected(&d.path);
        let biggest_child = scan.children[i].first().map(|&c| scan.dirs[c].total_bytes).unwrap_or(0);
        let concentrated = (biggest_child as f64) < (d.total_bytes as f64) * opts.concentration;
        if concentrated && !protected && i != scan.root_index && util::depth_from_root(&norm) >= 2 {
            folders.push(Candidate {
                id: 0,
                path: d.path.clone(),
                is_dir: true,
                size: d.total_bytes,
                file_count: d.file_count,
                modified: d.latest_mtime,
                source: Source::LargeFolder,
                category: "Large folders".into(),
                verdict: None,
                reason: String::new(),
                manual: None,
                protected: false,
                ai: None,
            });
            continue;
        }
        // A protected tree (e.g. Program Files) is never offered; skip it entirely
        // unless its children may be unprotected exceptions.
        if protected && guard.is_protected(&format!("{}\\__probe__", d.path)) {
            continue;
        }
        stack.extend(scan.children[i].iter().copied());
    }
    folders.sort_by(|a, b| b.size.cmp(&a.size));
    folders.truncate(opts.max_large_folders);
    // Large files that sit inside an offered folder are still listed on their own,
    // because "this one 8 GB file" is often the real answer.
    out.extend(folders);

    out.sort_by(|a, b| b.size.cmp(&a.size));
    for (i, c) in out.iter_mut().enumerate() {
        c.id = i as u32 + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{scan, Progress, ScanOptions};
    use std::sync::Arc;

    fn write(p: &std::path::Path, bytes: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn picks_rules_large_files_and_folders() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("proj/node_modules/a/x.js"), 3000);
        write(&r.join("proj/node_modules/b/y.js"), 3000);
        write(&r.join("media/raw/clip1.mov"), 4000);
        write(&r.join("media/raw/clip2.mov"), 4000);
        write(&r.join("big/one.bin"), 9000);
        write(&r.join("keep/Documents/report.pdf"), 9000);

        let res = scan(r, &ScanOptions { large_file_min: 8000, dup_min: 1 }, Arc::new(Progress::default())).unwrap();
        let docs = r.join("keep/Documents").to_string_lossy().into_owned();
        let guard = SafetyGuard::new(&[docs], &[], &[]);
        let opts = CandidateOptions {
            min_rule_bytes: 1000,
            min_large_file: 8000,
            min_large_folder: 7000,
            concentration: 0.6,
            max_large_files: 10,
            max_large_folders: 10,
        };
        let c = build_candidates(&res, &guard, &opts);
        let find = |suffix: &str| c.iter().find(|x| x.path.replace('\\', "/").ends_with(suffix));

        let nm = find("proj/node_modules").expect("node_modules found");
        assert_eq!(nm.source, Source::Rule);
        assert!(nm.can_recycle());
        assert!(find("node_modules/a").is_none(), "no nested duplicates");

        let raw = find("media/raw").expect("concentrated folder found");
        assert_eq!(raw.source, Source::LargeFolder);
        assert!(raw.verdict.is_none());

        let big = find("big/one.bin").expect("large file found");
        assert_eq!(big.source, Source::LargeFile);

        let report = find("Documents/report.pdf").expect("large file in protected folder is listed");
        assert!(report.protected);
        assert!(!report.can_recycle());
        assert!(find("keep/Documents").is_none(), "protected folders are never offered");

        // ids are unique and 1-based
        let mut ids: Vec<u32> = c.iter().map(|x| x.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), c.len());
        assert_eq!(c[0].id, 1);
    }
}
