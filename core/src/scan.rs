//! Parallel directory scanner.
//!
//! Walks the tree with rayon (one task per sub-folder) using `std::fs::read_dir`.
//! On Windows `DirEntry::metadata()` comes from the FindNextFile data, so no extra
//! system call per file is needed. Symlinks and junctions are skipped, which also
//! avoids the classic `Application Data -> AppData\Roaming` loops.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use crate::util;

/// Live counters shared with the UI while a scan runs.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub cancel: AtomicBool,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Files at least this big are kept individually ("large files").
    pub large_file_min: u64,
    /// Files at least this big are considered for duplicate detection.
    pub dup_min: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            large_file_min: 50 * 1024 * 1024,
            dup_min: 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DirRecord {
    pub path: String,
    pub depth: u32,
    /// Bytes in this folder and everything below it.
    pub total_bytes: u64,
    /// Bytes of files directly in this folder.
    pub own_bytes: u64,
    /// Files in this folder and below.
    pub file_count: u64,
    /// Newest modification time (unix secs) of any file below.
    pub latest_mtime: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileRecord {
    pub path: String,
    pub size: u64,
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtStat {
    pub ext: String,
    pub bytes: u64,
    pub files: u64,
}

pub struct ScanResult {
    pub root: String,
    pub dirs: Vec<DirRecord>,
    /// children[i] = indices of sub-folders of dirs[i], largest first.
    pub children: Vec<Vec<usize>>,
    /// normalized path -> index into `dirs`.
    pub index: HashMap<String, usize>,
    pub root_index: usize,
    /// Largest files, biggest first.
    pub large_files: Vec<FileRecord>,
    /// size -> paths, only sizes seen more than once (duplicate candidates).
    pub same_size: HashMap<u64, Vec<String>>,
    pub ext_stats: Vec<ExtStat>,
    pub total_bytes: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

#[derive(Default, Clone, Copy)]
struct Agg {
    bytes: u64,
    files: u64,
    latest: i64,
}

struct Ctx<'a> {
    opts: &'a ScanOptions,
    progress: &'a Progress,
    dirs: Mutex<Vec<DirRecord>>,
    large: Mutex<Vec<FileRecord>>,
    by_size: Mutex<HashMap<u64, Vec<String>>>,
    ext: Mutex<HashMap<String, (u64, u64)>>,
}

/// Scan `root` and everything below it. Blocks until done or cancelled.
pub fn scan(root: &Path, opts: &ScanOptions, progress: Arc<Progress>) -> Result<ScanResult, String> {
    let meta = fs::metadata(root).map_err(|e| format!("Cannot open {}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let started = Instant::now();
    let ctx = Ctx {
        opts,
        progress: &progress,
        dirs: Mutex::new(Vec::new()),
        large: Mutex::new(Vec::new()),
        by_size: Mutex::new(HashMap::new()),
        ext: Mutex::new(HashMap::new()),
    };

    // Deep trees + recursion: give worker threads a generous stack.
    let pool = rayon::ThreadPoolBuilder::new()
        .stack_size(16 * 1024 * 1024)
        .build()
        .map_err(|e| e.to_string())?;
    let total = pool.install(|| walk(root.to_path_buf(), 0, &ctx));

    let mut dirs = ctx.dirs.into_inner().unwrap();
    // Parents before children makes later tree walks simple.
    dirs.sort_by(|a, b| a.depth.cmp(&b.depth).then(b.total_bytes.cmp(&a.total_bytes)));

    let mut index = HashMap::with_capacity(dirs.len());
    for (i, d) in dirs.iter().enumerate() {
        index.insert(util::normalize(&d.path), i);
    }
    let mut children = vec![Vec::new(); dirs.len()];
    for (i, d) in dirs.iter().enumerate() {
        if d.depth == 0 {
            continue;
        }
        if let Some(p) = util::parent_of(&d.path) {
            if let Some(&pi) = index.get(&util::normalize(p)) {
                children[pi].push(i);
            }
        }
    }
    // dirs are sorted by size within a depth, so children are already largest first.
    let root_index = dirs.iter().position(|d| d.depth == 0).unwrap_or(0);

    let mut large = ctx.large.into_inner().unwrap();
    large.sort_by(|a, b| b.size.cmp(&a.size));
    large.truncate(2000);

    let mut same_size = ctx.by_size.into_inner().unwrap();
    same_size.retain(|_, v| v.len() > 1);

    let mut ext_stats: Vec<ExtStat> = ctx
        .ext
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|(ext, (bytes, files))| ExtStat { ext, bytes, files })
        .collect();
    ext_stats.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    ext_stats.truncate(40);

    let dir_count = dirs.len() as u64;
    Ok(ScanResult {
        root: root.to_string_lossy().into_owned(),
        dirs,
        children,
        index,
        root_index,
        large_files: large,
        same_size,
        ext_stats,
        total_bytes: total.bytes,
        file_count: total.files,
        dir_count,
        errors: progress.errors.load(Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
        cancelled: progress.cancel.load(Relaxed),
    })
}

fn walk(path: PathBuf, depth: u32, ctx: &Ctx) -> Agg {
    if ctx.progress.cancel.load(Relaxed) {
        return Agg::default();
    }
    let rd = match fs::read_dir(&path) {
        Ok(r) => r,
        Err(_) => {
            ctx.progress.errors.fetch_add(1, Relaxed);
            return Agg::default();
        }
    };

    let mut subdirs = Vec::new();
    let mut own = Agg::default();
    let mut large = Vec::new();
    let mut sized = Vec::new();
    let mut ext: HashMap<String, (u64, u64)> = HashMap::new();

    for entry in rd {
        let Ok(entry) = entry else {
            ctx.progress.errors.fetch_add(1, Relaxed);
            continue;
        };
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue; // symlinks and junctions: never follow, never count
        }
        if ft.is_dir() {
            subdirs.push(entry.path());
            continue;
        }
        let Ok(md) = entry.metadata() else {
            ctx.progress.errors.fetch_add(1, Relaxed);
            continue;
        };
        let size = disk_size(&md);
        let mtime = util::unix_secs(md.modified());
        own.bytes += size;
        own.files += 1;
        own.latest = own.latest.max(mtime);

        let name = entry.file_name();
        let name = name.to_string_lossy();
        let e = ext
            .entry(util::extension(&name).unwrap_or_else(|| "(none)".into()))
            .or_default();
        e.0 += size;
        e.1 += 1;

        if size >= ctx.opts.dup_min || size >= ctx.opts.large_file_min {
            let p = entry.path().to_string_lossy().into_owned();
            if size >= ctx.opts.large_file_min {
                large.push(FileRecord { path: p.clone(), size, modified: mtime });
            }
            if size >= ctx.opts.dup_min {
                sized.push((size, p));
            }
        }
    }

    ctx.progress.files.fetch_add(own.files, Relaxed);
    ctx.progress.bytes.fetch_add(own.bytes, Relaxed);
    ctx.progress.dirs.fetch_add(1, Relaxed);
    if !large.is_empty() {
        ctx.large.lock().unwrap().extend(large);
    }
    if !sized.is_empty() {
        let mut m = ctx.by_size.lock().unwrap();
        for (s, p) in sized {
            m.entry(s).or_default().push(p);
        }
    }
    if !ext.is_empty() {
        let mut m = ctx.ext.lock().unwrap();
        for (k, (b, f)) in ext {
            let e = m.entry(k).or_default();
            e.0 += b;
            e.1 += f;
        }
    }

    let kids: Vec<Agg> = subdirs
        .into_par_iter()
        .map(|p| walk(p, depth + 1, ctx))
        .collect();

    let mut total = own;
    for k in kids {
        total.bytes += k.bytes;
        total.files += k.files;
        total.latest = total.latest.max(k.latest);
    }

    ctx.dirs.lock().unwrap().push(DirRecord {
        path: path.to_string_lossy().into_owned(),
        depth,
        total_bytes: total.bytes,
        own_bytes: own.bytes,
        file_count: total.files,
        latest_mtime: total.latest,
    });
    total
}

/// Bytes a file really uses locally. OneDrive "online-only" placeholders report
/// their full size but take no space, so they count as 0.
#[cfg(windows)]
fn disk_size(md: &fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    let attrs = md.file_attributes();
    if attrs & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0 {
        0
    } else {
        md.len()
    }
}

#[cfg(not(windows))]
fn disk_size(md: &fs::Metadata) -> u64 {
    md.len()
}

/// One level of the folder tree, for the Explore screen.
#[derive(Debug, Clone, Serialize)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub file_count: u64,
    pub is_dir: bool,
    pub modified: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowseResult {
    pub path: String,
    pub parent: Option<String>,
    pub size: u64,
    pub entries: Vec<BrowseEntry>,
}

impl ScanResult {
    pub fn dir_index(&self, path: &str) -> Option<usize> {
        self.index.get(&util::normalize(path)).copied()
    }

    pub fn browse(&self, path: Option<&str>) -> Option<BrowseResult> {
        let i = match path {
            Some(p) => self.dir_index(p)?,
            None => self.root_index,
        };
        let d = self.dirs.get(i)?;
        let mut entries: Vec<BrowseEntry> = self.children[i]
            .iter()
            .map(|&c| {
                let cd = &self.dirs[c];
                BrowseEntry {
                    name: util::file_name(&cd.path).to_string(),
                    path: cd.path.clone(),
                    size: cd.total_bytes,
                    file_count: cd.file_count,
                    is_dir: true,
                    modified: cd.latest_mtime,
                }
            })
            .collect();
        // Large files that live directly in this folder.
        let norm = util::normalize(&d.path);
        let mut shown_file_bytes = 0;
        let mut shown_files = 0;
        for f in &self.large_files {
            if util::parent_of(&f.path).map(util::normalize).as_deref() == Some(norm.as_str()) {
                shown_file_bytes += f.size;
                shown_files += 1;
                entries.push(BrowseEntry {
                    name: util::file_name(&f.path).to_string(),
                    path: f.path.clone(),
                    size: f.size,
                    file_count: 1,
                    is_dir: false,
                    modified: f.modified,
                });
            }
        }
        let rest = d.own_bytes.saturating_sub(shown_file_bytes);
        if rest > 0 {
            entries.push(BrowseEntry {
                name: if shown_files > 0 { "(other files here)".into() } else { "(files here)".into() },
                path: String::new(),
                size: rest,
                file_count: 0,
                is_dir: false,
                modified: 0,
            });
        }
        entries.sort_by(|a, b| b.size.cmp(&a.size));
        let parent = if i == self.root_index {
            None
        } else {
            util::parent_of(&d.path).map(|s| s.to_string())
        };
        Some(BrowseResult { path: d.path.clone(), parent, size: d.total_bytes, entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = fs::File::create(p).unwrap();
        f.write_all(&vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn totals_tree_and_large_files() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("a/one.bin"), 3000);
        write(&r.join("a/b/two.bin"), 5000);
        write(&r.join("c/three.txt"), 100);
        write(&r.join("top.dat"), 10);

        let opts = ScanOptions { large_file_min: 4000, dup_min: 1 };
        let res = scan(r, &opts, Arc::new(Progress::default())).unwrap();
        assert_eq!(res.total_bytes, 8110);
        assert_eq!(res.file_count, 4);
        assert_eq!(res.dir_count, 4); // root, a, a/b, c
        let a = res.dir_index(&r.join("a").to_string_lossy()).unwrap();
        assert_eq!(res.dirs[a].total_bytes, 8000);
        assert_eq!(res.dirs[a].own_bytes, 3000);
        assert_eq!(res.large_files.len(), 1);
        assert_eq!(res.large_files[0].size, 5000);

        let root = res.browse(None).unwrap();
        assert_eq!(root.entries[0].name, "a");
        assert!(root.parent.is_none());
        let sub = res.browse(Some(&r.join("a").to_string_lossy())).unwrap();
        assert_eq!(sub.entries.len(), 2); // folder b + files here
        assert!(res.ext_stats.iter().any(|e| e.ext == "bin" && e.bytes == 8000));
    }

    #[test]
    fn same_size_groups() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("x/1.bin"), 2048);
        write(&t.path().join("y/2.bin"), 2048);
        write(&t.path().join("y/3.bin"), 999);
        let opts = ScanOptions { large_file_min: u64::MAX, dup_min: 1 };
        let res = scan(t.path(), &opts, Arc::new(Progress::default())).unwrap();
        assert_eq!(res.same_size.len(), 1);
        assert_eq!(res.same_size[&2048].len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlink_loops() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("d/f.bin"), 10);
        std::os::unix::fs::symlink(t.path(), t.path().join("d/loop")).unwrap();
        let res = scan(t.path(), &ScanOptions::default(), Arc::new(Progress::default())).unwrap();
        assert_eq!(res.file_count, 1);
    }
}
