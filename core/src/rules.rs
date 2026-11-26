//! Rule engine: recognises well-known space-eaters without any AI.
//!
//! Every rule works on the normalized path (lower-case, backslashes), so the same
//! rules are testable on any OS with Windows-style strings.

use std::path::Path;

use serde::Serialize;

use crate::util;
use crate::Verdict;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RuleHit {
    pub rule: &'static str,
    pub category: &'static str,
    pub verdict: Verdict,
    pub reason: &'static str,
    /// Set when the space should be reclaimed some other way than the Recycle Bin.
    /// The UI shows this hint and does not allow selecting the item.
    pub manual: Option<&'static str>,
}

const fn hit(
    rule: &'static str,
    category: &'static str,
    verdict: Verdict,
    reason: &'static str,
    manual: Option<&'static str>,
) -> RuleHit {
    RuleHit { rule, category, verdict, reason, manual }
}

/// Context passed to folder rules.
pub struct DirInfo<'a> {
    /// Normalized path.
    pub norm: &'a str,
    /// Original path (for file-system checks such as "is there a Cargo.toml next to it").
    pub path: &'a str,
    pub depth_from_drive: usize,
}

pub fn depth_from_drive(norm: &str) -> usize {
    norm.trim_end_matches('\\').matches('\\').count()
}

fn ends(norm: &str, suffix: &str) -> bool {
    norm.ends_with(suffix)
}

const APP_CACHE_NAMES: &[&str] = &[
    "cache", "cache2", "code cache", "gpucache", "cache_data", "shadercache",
    "grshadercache", "dawncache", "dawngraphitecache", "dawnwebgpucache", "service worker",
];

pub fn classify_dir(d: &DirInfo) -> Option<RuleHit> {
    use Verdict::*;
    let n = d.norm;
    let name = util::file_name(n);

    // ---- things to leave alone (shown so you know what they are) ----
    if ends(n, "\\windows\\winsxs") {
        return Some(hit("winsxs", "Windows components", Keep,
            "Windows component store. Deleting files here breaks Windows.",
            Some("Run as admin: DISM /Online /Cleanup-Image /StartComponentCleanup")));
    }
    if ends(n, "\\windows\\installer") {
        return Some(hit("win-installer", "Windows components", Keep,
            "Needed to repair and uninstall installed programs.", None));
    }
    if name == "steamapps" {
        return Some(hit("steam", "Games", Keep,
            "Installed Steam games.", Some("Uninstall games you no longer play from Steam.")));
    }

    // ---- Windows clean-up that must go through Windows tools ----
    if name == "windows.old" && d.depth_from_drive == 1 {
        return Some(hit("windows-old", "Windows update leftovers", Caution,
            "Copy of your previous Windows install, kept for rollback.",
            Some("Settings > System > Storage > Temporary files > Previous Windows installation(s).")));
    }
    if ends(n, "\\windows\\softwaredistribution\\download") {
        return Some(hit("wu-download", "Windows update leftovers", Safe,
            "Downloaded Windows Update packages.",
            Some("Settings > System > Storage > Temporary files > Windows Update Cleanup.")));
    }
    if name == "$recycle.bin" {
        return Some(hit("recycle-bin", "Recycle Bin", Safe,
            "Files you already deleted.", Some("Right-click the Recycle Bin > Empty Recycle Bin.")));
    }
    if ends(n, "\\docker\\wsl") || ends(n, "\\docker\\wsl\\data") {
        return Some(hit("docker", "Virtual disks", Caution,
            "Docker Desktop's virtual disk (images, containers, volumes).",
            Some("Run: docker system prune -a  (then compact the disk in Docker Desktop).")));
    }

    // ---- temp & caches ----
    if ends(n, "\\appdata\\local\\temp") || ends(n, "\\windows\\temp") {
        return Some(hit("temp", "Temporary files", Safe,
            "Temporary files. Apps recreate what they still need.", None));
    }
    if ends(n, "\\microsoft\\windows\\inetcache") || ends(n, "\\microsoft\\windows\\inetcookies\\low") {
        return Some(hit("inetcache", "Browser & app caches", Safe, "Internet cache.", None));
    }
    if name == "crashdumps" || ends(n, "\\windows\\minidump") || ends(n, "\\windows\\livekernelreports")
        || ends(n, "\\microsoft\\windows\\wer\\reportarchive") || ends(n, "\\microsoft\\windows\\wer\\reportqueue")
    {
        return Some(hit("crash", "Crash dumps & error reports", Safe,
            "Crash dumps and error reports. Only useful for debugging a past crash.", None));
    }
    if ends(n, "\\nvidia\\dxcache") || ends(n, "\\nvidia\\glcache") || ends(n, "\\nvidia corporation\\nv_cache")
        || ends(n, "\\appdata\\local\\d3dscache") || ends(n, "\\amd\\dxcache")
    {
        return Some(hit("shader", "Browser & app caches", Safe,
            "GPU shader cache. Rebuilt automatically (games may stutter briefly once).", None));
    }
    if ends(n, "\\appdata\\local\\npm-cache") || ends(n, "\\appdata\\roaming\\npm-cache") {
        return Some(hit("npm-cache", "Developer caches", Safe, "npm download cache. Refilled on demand.", Some("Or run: npm cache clean --force")));
    }
    if ends(n, "\\appdata\\local\\yarn\\cache") || ends(n, "\\appdata\\local\\pnpm-cache")
        || ends(n, "\\appdata\\local\\pip\\cache") || ends(n, "\\appdata\\local\\uv\\cache")
    {
        return Some(hit("pkg-cache", "Developer caches", Safe, "Package manager download cache. Refilled on demand.", None));
    }
    if ends(n, "\\.gradle\\caches") || ends(n, "\\.nuget\\packages") || ends(n, "\\.cargo\\registry")
        || ends(n, "\\.m2\\repository")
    {
        return Some(hit("dev-store", "Developer caches", Caution,
            "Shared package store. Re-downloaded on the next build, which can be slow.", None));
    }
    if APP_CACHE_NAMES.contains(&name) && n.contains("\\appdata\\") {
        return Some(hit("app-cache", "Browser & app caches", Safe,
            "App or browser cache. Close the app first; it rebuilds the cache.", None));
    }

    // ---- project build output ----
    if name == "node_modules" {
        return Some(hit("node-modules", "Project dependencies", Safe,
            "JavaScript dependencies. `npm install` restores them.", None));
    }
    if matches!(name, "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".tox") {
        return Some(hit("py-cache", "Build output", Safe, "Python cache files. Recreated automatically.", None));
    }
    if matches!(name, ".next" | ".nuxt" | ".parcel-cache" | ".turbo" | ".angular" | ".svelte-kit") {
        return Some(hit("js-build", "Build output", Safe, "Framework build cache. Recreated on next build.", None));
    }
    if name == "target" {
        if let Some(parent) = util::parent_of(d.path) {
            if Path::new(parent).join("Cargo.toml").exists() {
                return Some(hit("rust-target", "Build output", Safe,
                    "Rust build output. `cargo build` recreates it.", None));
            }
        }
    }
    if matches!(name, "bin" | "obj") {
        if let Some(parent) = util::parent_of(d.path) {
            if has_ext_file(parent, &["csproj", "fsproj", "vbproj"]) {
                return Some(hit("dotnet-build", "Build output", Safe,
                    ".NET build output. Rebuilt on next build.", None));
            }
        }
    }
    None
}

fn has_ext_file(dir: &str, exts: &[&str]) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| {
        util::extension(&e.file_name().to_string_lossy())
            .map(|x| exts.contains(&x.as_str()))
            .unwrap_or(false)
    })
}

pub fn classify_file(norm: &str) -> Option<RuleHit> {
    use Verdict::*;
    let name = util::file_name(norm);
    let at_root = depth_from_drive(norm) == 1;
    let ext = util::extension(name).unwrap_or_default();

    if at_root && name == "hiberfil.sys" {
        return Some(hit("hiberfil", "Windows system files", Keep,
            "Hibernation file, about the size of your RAM.",
            Some("If you never hibernate: run `powercfg /h off` as admin (also disables Fast Startup).")));
    }
    if at_root && (name == "pagefile.sys" || name == "swapfile.sys") {
        return Some(hit("pagefile", "Windows system files", Keep,
            "Virtual memory used by Windows.", Some("Size can be changed in System > Advanced > Performance > Virtual memory.")));
    }
    if ext == "vhd" || ext == "vhdx" {
        return Some(hit("vdisk", "Virtual disks", Caution,
            "Virtual disk (WSL, Hyper-V, Docker or a VM).",
            Some("Remove it through the tool that owns it (wsl --unregister, Hyper-V Manager, Docker).")));
    }
    if ext == "dmp" || ext == "mdmp" {
        return Some(hit("dump-file", "Crash dumps & error reports", Safe, "Crash dump file.", None));
    }
    if ext == "tmp" {
        return Some(hit("tmp-file", "Temporary files", Safe, "Temporary file.", None));
    }
    if norm.contains("\\downloads\\")
        && matches!(ext.as_str(), "exe" | "msi" | "msix" | "msixbundle" | "zip" | "7z" | "rar" | "iso")
    {
        return Some(hit("downloads-installer", "Old downloads", Caution,
            "Installer or archive in Downloads. Usually safe once installed or extracted.", None));
    }
    if matches!(ext.as_str(), "iso" | "img") {
        return Some(hit("disk-image", "Disk images", Caution, "Disk image. Safe if you can download it again.", None));
    }
    if matches!(ext.as_str(), "bak" | "old") {
        return Some(hit("backup-file", "Backups", Caution, "Backup copy of another file.", None));
    }
    if ext == "log" || ext == "etl" {
        return Some(hit("log-file", "Logs", Safe, "Log file.", None));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(p: &str) -> Option<RuleHit> {
        let norm = util::normalize(p);
        classify_dir(&DirInfo { norm: &norm, path: p, depth_from_drive: depth_from_drive(&norm) })
    }

    #[test]
    fn folder_rules() {
        assert_eq!(dir("C:\\Users\\Bob\\AppData\\Local\\Temp").unwrap().rule, "temp");
        assert_eq!(dir("D:\\code\\site\\node_modules").unwrap().rule, "node-modules");
        assert_eq!(dir("C:\\Windows.old").unwrap().rule, "windows-old");
        assert!(dir("C:\\stuff\\Windows.old").is_none());
        let chrome = "C:\\Users\\Bob\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Cache";
        assert_eq!(dir(chrome).unwrap().rule, "app-cache");
        assert!(dir("C:\\Users\\Bob\\Documents\\cache").is_none(), "cache outside AppData is not assumed");
        assert_eq!(dir("C:\\Windows\\WinSxS").unwrap().verdict, Verdict::Keep);
        assert!(dir("C:\\Users\\Bob\\Pictures").is_none());
    }

    #[test]
    fn rust_target_needs_cargo_toml() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("target");
        std::fs::create_dir_all(&target).unwrap();
        let p = target.to_string_lossy().into_owned();
        let norm = util::normalize(&p);
        let info = DirInfo { norm: &norm, path: &p, depth_from_drive: 3 };
        assert!(classify_dir(&info).is_none());
        std::fs::write(t.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(classify_dir(&info).unwrap().rule, "rust-target");
    }

    #[test]
    fn file_rules() {
        assert_eq!(classify_file("c:\\hiberfil.sys").unwrap().verdict, Verdict::Keep);
        assert!(classify_file("c:\\x\\hiberfil.sys").is_none());
        assert_eq!(classify_file("c:\\users\\bob\\downloads\\setup.exe").unwrap().rule, "downloads-installer");
        assert_eq!(classify_file("d:\\isos\\ubuntu.iso").unwrap().rule, "disk-image");
        assert!(classify_file("c:\\users\\bob\\videos\\holiday.mp4").is_none());
    }
}
