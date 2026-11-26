mod ai;
mod settings;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use disksage_core::candidates::{build_candidates, Candidate, CandidateOptions};
use disksage_core::duplicates::{find_duplicates as find_dups, DupGroup};
use disksage_core::safety::SafetyGuard;
use disksage_core::scan::{self, BrowseResult, ExtStat, Progress, ScanOptions, ScanResult};
use disksage_core::Verdict;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use settings::{Settings, SettingsView};

#[derive(Default)]
struct AppState {
    scan: Mutex<Option<Arc<ScanResult>>>,
    candidates: Mutex<Vec<Candidate>>,
    progress: Mutex<Option<Arc<Progress>>>,
    dup_cancel: Arc<AtomicBool>,
    busy: AtomicBool,
    settings: Mutex<Settings>,
    settings_path: Mutex<PathBuf>,
    http: reqwest::Client,
}

fn guard() -> SafetyGuard {
    SafetyGuard::from_env()
}

// ---------------------------------------------------------------- drives

#[derive(Serialize)]
struct DriveInfo {
    mount: String,
    name: String,
    total: u64,
    free: u64,
}

#[tauri::command]
fn list_drives() -> Vec<DriveInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut out: Vec<DriveInfo> = disks
        .list()
        .iter()
        .map(|d| DriveInfo {
            mount: d.mount_point().to_string_lossy().into_owned(),
            name: d.name().to_string_lossy().into_owned(),
            total: d.total_space(),
            free: d.available_space(),
        })
        .filter(|d| d.total > 0)
        .collect();
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out.dedup_by(|a, b| a.mount == b.mount);
    out
}

// ---------------------------------------------------------------- scanning

#[derive(Serialize, Clone)]
struct ProgressEvent {
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    elapsed_ms: u64,
}

#[derive(Serialize)]
struct ScanSummary {
    root: String,
    total_bytes: u64,
    file_count: u64,
    dir_count: u64,
    errors: u64,
    elapsed_ms: u64,
    cancelled: bool,
    ext_stats: Vec<ExtStat>,
    candidate_count: usize,
    reclaimable_safe: u64,
    duplicate_sizes: usize,
}

struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Relaxed);
    }
}

fn claim_busy(state: &AppState) -> Result<BusyGuard<'_>, String> {
    if state.busy.swap(true, Relaxed) {
        return Err("Another scan or analysis is already running.".into());
    }
    Ok(BusyGuard(&state.busy))
}

#[tauri::command]
async fn start_scan(app: AppHandle, state: State<'_, AppState>, root: String) -> Result<ScanSummary, String> {
    let _busy = claim_busy(&state)?;
    let progress = Arc::new(Progress::default());
    *state.progress.lock().unwrap() = Some(progress.clone());

    // Report progress to the UI five times a second.
    let done = Arc::new(AtomicBool::new(false));
    {
        let (app, progress, done) = (app.clone(), progress.clone(), done.clone());
        std::thread::spawn(move || {
            let started = Instant::now();
            while !done.load(Relaxed) {
                let _ = app.emit("scan-progress", ProgressEvent {
                    files: progress.files.load(Relaxed),
                    dirs: progress.dirs.load(Relaxed),
                    bytes: progress.bytes.load(Relaxed),
                    errors: progress.errors.load(Relaxed),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
                std::thread::sleep(Duration::from_millis(200));
            }
        });
    }

    let root_path = PathBuf::from(&root);
    let p2 = progress.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let res = scan::scan(&root_path, &ScanOptions::default(), p2)?;
        let cands = build_candidates(&res, &guard(), &CandidateOptions::default());
        Ok::<_, String>((res, cands))
    })
    .await
    .map_err(|e| e.to_string());
    done.store(true, Relaxed);
    let (res, cands) = result??;

    let reclaimable_safe = cands
        .iter()
        .filter(|c| c.can_recycle() && c.effective_verdict() == Some(Verdict::Safe))
        .map(|c| c.size)
        .sum();
    let summary = ScanSummary {
        root: res.root.clone(),
        total_bytes: res.total_bytes,
        file_count: res.file_count,
        dir_count: res.dir_count,
        errors: res.errors,
        elapsed_ms: res.elapsed_ms,
        cancelled: res.cancelled,
        ext_stats: res.ext_stats.iter().take(15).cloned().collect(),
        candidate_count: cands.len(),
        reclaimable_safe,
        duplicate_sizes: res.same_size.len(),
    };
    *state.scan.lock().unwrap() = Some(Arc::new(res));
    *state.candidates.lock().unwrap() = cands;
    Ok(summary)
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    if let Some(p) = state.progress.lock().unwrap().as_ref() {
        p.cancel.store(true, Relaxed);
    }
    state.dup_cancel.store(true, Relaxed);
}

#[tauri::command]
fn browse(state: State<'_, AppState>, path: Option<String>) -> Result<BrowseResult, String> {
    let scan = state.scan.lock().unwrap().clone().ok_or("Run a scan first.")?;
    scan.browse(path.as_deref()).ok_or_else(|| "Folder not found in the last scan.".into())
}

#[tauri::command]
fn get_candidates(state: State<'_, AppState>) -> Vec<Candidate> {
    state.candidates.lock().unwrap().clone()
}

// ---------------------------------------------------------------- AI

#[derive(Serialize, Clone)]
struct AiProgress {
    done: usize,
    total: usize,
}

/// Ask Claude about the given candidates (or, with no ids, every candidate
/// that has no verdict yet). Returns the full, updated candidate list.
#[tauri::command]
async fn analyze_with_ai(app: AppHandle, state: State<'_, AppState>, ids: Vec<u32>) -> Result<Vec<Candidate>, String> {
    let _busy = claim_busy(&state)?;
    let settings = state.settings.lock().unwrap().clone();
    let todo: Vec<Candidate> = {
        let all = state.candidates.lock().unwrap();
        all.iter()
            .filter(|c| !c.protected)
            .filter(|c| if ids.is_empty() { c.verdict.is_none() && c.ai.is_none() } else { ids.contains(&c.id) })
            .cloned()
            .collect()
    };
    if todo.is_empty() {
        return Ok(state.candidates.lock().unwrap().clone());
    }

    let total = todo.len();
    let mut done = 0;
    let mut first_error = None;
    let _ = app.emit("ai-progress", AiProgress { done, total });
    for batch in todo.chunks(settings.batch_size) {
        match ai::analyze_batch(&state.http, &settings, batch).await {
            Ok(notes) => {
                let mut all = state.candidates.lock().unwrap();
                for c in all.iter_mut() {
                    if let Some(n) = notes.get(&c.id) {
                        c.ai = Some(n.clone());
                    }
                }
            }
            Err(e) => {
                // Stop on auth / config errors; they will fail for every batch.
                first_error = Some(e);
                break;
            }
        }
        done += batch.len();
        let _ = app.emit("ai-progress", AiProgress { done, total });
    }
    let all = state.candidates.lock().unwrap().clone();
    match first_error {
        Some(e) if done == 0 => Err(e),
        Some(e) => {
            let _ = app.emit("ai-warning", e);
            Ok(all)
        }
        None => Ok(all),
    }
}

// ---------------------------------------------------------------- duplicates

#[derive(Serialize, Clone)]
struct DupProgress {
    checked: u64,
    total: u64,
}

#[derive(Serialize)]
struct DupFile {
    path: String,
    modified: i64,
    protected: bool,
}

#[derive(Serialize)]
struct DupView {
    size: u64,
    hash: String,
    wasted: u64,
    files: Vec<DupFile>,
}

fn dup_view(g: DupGroup, guard: &SafetyGuard) -> DupView {
    let mut files: Vec<DupFile> = g
        .paths
        .into_iter()
        .map(|p| DupFile {
            modified: std::fs::metadata(&p).ok().map(|m| disksage_core::util::unix_secs(m.modified())).unwrap_or(0),
            protected: guard.is_protected(&p),
            path: p,
        })
        .collect();
    // Oldest first: the original is usually the one to keep.
    files.sort_by_key(|f| f.modified);
    DupView { size: g.size, hash: g.hash, wasted: g.wasted, files }
}

#[tauri::command]
async fn find_duplicates(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<DupView>, String> {
    let _busy = claim_busy(&state)?;
    let scan = state.scan.lock().unwrap().clone().ok_or("Run a scan first.")?;
    let cancel = state.dup_cancel.clone();
    cancel.store(false, Relaxed);
    let checked = Arc::new(AtomicU64::new(0));
    let total: u64 = scan.same_size.values().map(|v| v.len() as u64).sum();

    let done = Arc::new(AtomicBool::new(false));
    {
        let (app, checked, done) = (app.clone(), checked.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Relaxed) {
                let _ = app.emit("dup-progress", DupProgress { checked: checked.load(Relaxed), total });
                std::thread::sleep(Duration::from_millis(250));
            }
        });
    }
    let c2 = checked.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let guard = guard();
        find_dups(&scan.same_size, &cancel, &c2, 500)
            .into_iter()
            .map(|g| dup_view(g, &guard))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string());
    done.store(true, Relaxed);
    res
}

// ---------------------------------------------------------------- deleting

#[derive(Serialize)]
struct RecycleFailure {
    path: String,
    error: String,
}

#[derive(Serialize)]
struct RecycleReport {
    moved: Vec<String>,
    failed: Vec<RecycleFailure>,
    freed: u64,
}

/// Move items to the Recycle Bin. Every path is re-checked against the safety
/// guard here, so the UI can never delete a protected path, even by mistake.
#[tauri::command]
async fn recycle(state: State<'_, AppState>, paths: Vec<String>) -> Result<RecycleReport, String> {
    let sizes: HashMap<String, u64> = {
        let cands = state.candidates.lock().unwrap();
        let mut m: HashMap<String, u64> = cands.iter().map(|c| (c.path.clone(), c.size)).collect();
        if let Some(scan) = state.scan.lock().unwrap().as_ref() {
            for (size, ps) in &scan.same_size {
                for p in ps {
                    m.entry(p.clone()).or_insert(*size);
                }
            }
        }
        m
    };
    let blocked: HashMap<String, bool> = {
        let cands = state.candidates.lock().unwrap();
        cands.iter().map(|c| (c.path.clone(), !c.can_recycle())).collect()
    };

    let report = tauri::async_runtime::spawn_blocking(move || {
        let guard = guard();
        let mut report = RecycleReport { moved: vec![], failed: vec![], freed: 0 };
        for p in paths {
            let fail = |error: String| RecycleFailure { path: p.clone(), error };
            if let Err(e) = guard.check(&p) {
                report.failed.push(fail(e));
                continue;
            }
            if blocked.get(&p).copied().unwrap_or(false) {
                report.failed.push(fail("Marked keep or manual-only".into()));
                continue;
            }
            if !Path::new(&p).exists() {
                report.failed.push(fail("No longer exists".into()));
                continue;
            }
            match trash::delete(&p) {
                Ok(()) => {
                    report.freed += sizes.get(&p).copied().unwrap_or(0);
                    report.moved.push(p);
                }
                Err(e) => report.failed.push(fail(e.to_string())),
            }
        }
        report
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut cands = state.candidates.lock().unwrap();
    cands.retain(|c| !report.moved.contains(&c.path));
    Ok(report)
}

#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let dir = if Path::new(&path).is_dir() { PathBuf::from(&path) } else { Path::new(&path).parent().map(Path::to_path_buf).unwrap_or_default() };
        std::process::Command::new("xdg-open").arg(dir).spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[tauri::command]
fn check_path(path: String) -> Result<(), String> {
    guard().check(&path)
}

// ---------------------------------------------------------------- settings

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> SettingsView {
    let path = state.settings_path.lock().unwrap().clone();
    state.settings.lock().unwrap().view(&path)
}

#[tauri::command]
fn save_settings(
    state: State<'_, AppState>,
    api_key: Option<String>,
    model: String,
    redact_username: bool,
) -> Result<SettingsView, String> {
    let path = state.settings_path.lock().unwrap().clone();
    let mut s = state.settings.lock().unwrap();
    if let Some(k) = api_key {
        s.api_key = k.trim().to_string();
    }
    s.model = if model.trim().is_empty() { settings::DEFAULT_MODEL.into() } else { model.trim().into() };
    s.redact_username = redact_username;
    s.save(&path)?;
    Ok(s.view(&path))
}

#[tauri::command]
async fn test_ai(state: State<'_, AppState>) -> Result<String, String> {
    let s = state.settings.lock().unwrap().clone();
    ai::test_connection(&state.http, &s).await
}

// ---------------------------------------------------------------- app

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            let path = dir.join("settings.json");
            let state = app.state::<AppState>();
            *state.settings.lock().unwrap() = Settings::load(&path);
            *state.settings_path.lock().unwrap() = path;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_drives,
            start_scan,
            cancel,
            browse,
            get_candidates,
            analyze_with_ai,
            find_duplicates,
            recycle,
            reveal,
            check_path,
            get_settings,
            save_settings,
            test_ai,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DiskSage");
}
