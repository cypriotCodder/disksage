//! Safety guard: paths DiskSage will never delete, whatever a rule or the AI says.
//!
//! This is checked twice: when candidates are built (protected items are always
//! shown as "Keep") and again right before anything is moved to the Recycle Bin.

use crate::util::{self, is_within};

#[derive(Debug, Clone)]
pub struct SafetyGuard {
    /// Nothing inside these folders may be deleted.
    protected_trees: Vec<String>,
    /// These exact folders may not be deleted, but things inside them can be.
    protected_exact: Vec<String>,
    /// Exceptions inside protected trees (e.g. C:\Windows\Temp).
    allowed_trees: Vec<String>,
}

impl SafetyGuard {
    pub fn new(protected_trees: &[String], protected_exact: &[String], allowed_trees: &[String]) -> Self {
        let norm = |v: &[String]| v.iter().filter(|s| !s.is_empty()).map(|s| util::normalize(s)).collect();
        Self {
            protected_trees: norm(protected_trees),
            protected_exact: norm(protected_exact),
            allowed_trees: norm(allowed_trees),
        }
    }

    /// Build the guard from this machine's environment variables.
    pub fn from_env() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let windir = {
            let w = env("SystemRoot");
            if w.is_empty() { "C:\\Windows".to_string() } else { w }
        };
        let home = env("USERPROFILE");
        let mut trees = vec![
            windir.clone(),
            env("ProgramFiles"),
            env("ProgramFiles(x86)"),
            env("ProgramW6432"),
            format!("{}\\Microsoft", env("ProgramData")),
            "C:\\Program Files".into(),
            "C:\\Program Files (x86)".into(),
            "C:\\Recovery".into(),
            "C:\\System Volume Information".into(),
            "C:\\Boot".into(),
        ];
        let mut exact = vec![
            "C:\\Users".to_string(),
            env("ProgramData"),
            env("PUBLIC"),
        ];
        if !home.is_empty() {
            for sub in ["Documents", "Desktop", "Pictures", "Videos", "Music", "OneDrive", "Favorites", "Contacts", "Saved Games"] {
                trees.push(format!("{home}\\{sub}"));
            }
            for sub in ["", "AppData", "AppData\\Local", "AppData\\Roaming", "AppData\\LocalLow", "Downloads"] {
                exact.push(if sub.is_empty() { home.clone() } else { format!("{home}\\{sub}") });
            }
        }
        let allowed = vec![format!("{windir}\\Temp")];
        Self::new(&trees, &exact, &allowed)
    }

    /// `Err(reason)` if `path` must not be deleted.
    pub fn check(&self, path: &str) -> Result<(), String> {
        let n = util::normalize(path);
        if n.is_empty() {
            return Err("Empty path".into());
        }
        if is_drive_root(&n) {
            return Err("Drive roots are protected".into());
        }
        if util::depth_from_root(&n) == 1 && !n.ends_with("$recycle.bin") {
            // Anything directly at a drive root (pagefile, bootmgr, top-level folders).
            return Err("Top-level drive items are protected".into());
        }
        if n.split('\\').any(|c| c == ".git") {
            return Err("Git repositories are protected".into());
        }
        if self.allowed_trees.iter().any(|a| is_within(&n, a)) {
            return Ok(());
        }
        if let Some(p) = self.protected_trees.iter().find(|p| is_within(&n, p)) {
            return Err(format!("Inside a protected folder ({p})"));
        }
        if self.protected_exact.iter().any(|p| *p == n) {
            return Err("This folder itself is protected".into());
        }
        Ok(())
    }

    pub fn is_protected(&self, path: &str) -> bool {
        self.check(path).is_err()
    }
}

fn is_drive_root(n: &str) -> bool {
    n == "\\" || n == "/" || (n.len() <= 3 && n.as_bytes().get(1) == Some(&b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> SafetyGuard {
        SafetyGuard::new(
            &["C:\\Windows".into(), "C:\\Program Files".into(), "C:\\Users\\Bob\\Documents".into()],
            &["C:\\Users\\Bob".into(), "C:\\Users\\Bob\\AppData\\Local".into()],
            &["C:\\Windows\\Temp".into()],
        )
    }

    #[test]
    fn protects_system_and_personal_folders() {
        let g = guard();
        assert!(g.is_protected("C:\\"));
        assert!(g.is_protected("C:\\Windows\\System32"));
        assert!(g.is_protected("c:/windows"));
        assert!(g.is_protected("C:\\Program Files\\App\\cache"));
        assert!(g.is_protected("C:\\Users\\Bob\\Documents\\thesis.docx"));
        assert!(g.is_protected("C:\\Users\\Bob"));
        assert!(g.is_protected("C:\\Users\\Bob\\AppData\\Local"));
        assert!(g.is_protected("C:\\pagefile.sys"));
        assert!(g.is_protected("D:\\code\\app\\.git\\objects"));
    }

    #[test]
    fn allows_caches_and_exceptions() {
        let g = guard();
        assert!(!g.is_protected("C:\\Windows\\Temp"));
        assert!(!g.is_protected("C:\\Windows\\Temp\\x.tmp"));
        assert!(!g.is_protected("C:\\Users\\Bob\\AppData\\Local\\Temp"));
        assert!(!g.is_protected("D:\\code\\app\\node_modules"));
        assert!(!g.is_protected("C:\\WindowsApps2\\x"), "prefix match is per path component");
    }
}
