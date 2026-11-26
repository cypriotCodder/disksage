//! Duplicate finder: same size -> same partial hash (start + end) -> same full hash.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

use rayon::prelude::*;
use serde::Serialize;
use xxhash_rust::xxh3::{xxh3_64, Xxh3};

#[derive(Debug, Clone, Serialize)]
pub struct DupGroup {
    pub size: u64,
    pub hash: String,
    pub paths: Vec<String>,
    /// Space freed if all but one copy are removed.
    pub wasted: u64,
}

const EDGE: usize = 64 * 1024;

fn partial_hash(path: &str, size: u64) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; EDGE];
    let n = read_full(&mut f, &mut buf)?;
    let mut data = buf[..n].to_vec();
    if size > (EDGE as u64) * 2 {
        f.seek(SeekFrom::End(-(EDGE as i64))).ok()?;
        let n = read_full(&mut f, &mut buf)?;
        data.extend_from_slice(&buf[..n]);
    }
    Some(xxh3_64(&data))
}

fn full_hash(path: &str, cancel: &AtomicBool) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        if cancel.load(Relaxed) {
            return None;
        }
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.digest())
}

fn read_full(f: &mut File, buf: &mut [u8]) -> Option<usize> {
    let mut total = 0;
    while total < buf.len() {
        match f.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => return None,
        }
    }
    Some(total)
}

fn group_by<F>(paths: &[String], f: F) -> Vec<(u64, Vec<String>)>
where
    F: Fn(&str) -> Option<u64> + Sync,
{
    let hashed: Vec<(u64, String)> = paths
        .par_iter()
        .filter_map(|p| f(p).map(|h| (h, p.clone())))
        .collect();
    let mut m: HashMap<u64, Vec<String>> = HashMap::new();
    for (h, p) in hashed {
        m.entry(h).or_default().push(p);
    }
    m.into_iter().filter(|(_, v)| v.len() > 1).collect()
}

/// `same_size` comes from the scan. `checked` counts files hashed so far (for progress).
pub fn find_duplicates(
    same_size: &HashMap<u64, Vec<String>>,
    cancel: &AtomicBool,
    checked: &AtomicU64,
    max_groups: usize,
) -> Vec<DupGroup> {
    let mut sizes: Vec<(&u64, &Vec<String>)> = same_size.iter().collect();
    // Biggest potential savings first, so a cancelled run still found the best ones.
    sizes.sort_by(|a, b| (b.0 * (b.1.len() as u64 - 1)).cmp(&(a.0 * (a.1.len() as u64 - 1))));

    let mut out = Vec::new();
    for (&size, paths) in sizes {
        if cancel.load(Relaxed) {
            break;
        }
        for (_, same_partial) in group_by(paths, |p| partial_hash(p, size)) {
            let full = if size as usize <= EDGE * 2 {
                // Partial hash already covered the whole file.
                vec![(0, same_partial)]
            } else {
                group_by(&same_partial, |p| full_hash(p, cancel))
            };
            for (h, mut group) in full {
                if group.len() < 2 {
                    continue;
                }
                group.sort();
                out.push(DupGroup {
                    size,
                    hash: format!("{h:016x}"),
                    wasted: size * (group.len() as u64 - 1),
                    paths: group,
                });
            }
        }
        checked.fetch_add(paths.len() as u64, Relaxed);
    }
    out.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    out.truncate(max_groups);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_real_duplicates_only() {
        let t = tempfile::tempdir().unwrap();
        let p = |n: &str| t.path().join(n).to_string_lossy().into_owned();
        let big_a = vec![1u8; 300_000];
        let mut big_b = big_a.clone();
        big_b[150_000] = 2; // same size, same edges, different middle
        std::fs::write(p("a1"), &big_a).unwrap();
        std::fs::write(p("a2"), &big_a).unwrap();
        std::fs::write(p("b"), &big_b).unwrap();
        std::fs::write(p("s1"), b"hello").unwrap();
        std::fs::write(p("s2"), b"hello").unwrap();

        let mut m = HashMap::new();
        m.insert(300_000u64, vec![p("a1"), p("a2"), p("b")]);
        m.insert(5u64, vec![p("s1"), p("s2")]);
        let groups = find_duplicates(&m, &AtomicBool::new(false), &AtomicU64::new(0), 10);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].paths, vec![p("a1"), p("a2")]);
        assert_eq!(groups[0].wasted, 300_000);
    }
}
