use std::time::{SystemTime, UNIX_EPOCH};

/// Lower-case, backslash-separated form used for every path comparison.
/// Windows paths are case-insensitive, so all matching happens on this form.
pub fn normalize(path: &str) -> String {
    let mut s = path.replace('/', "\\").to_lowercase();
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// `child` is `parent` itself or lies somewhere inside it (both normalized).
pub fn is_within(child: &str, parent: &str) -> bool {
    if child == parent {
        return true;
    }
    if !child.starts_with(parent) {
        return false;
    }
    parent.ends_with('\\') || child.as_bytes().get(parent.len()) == Some(&b'\\')
}

/// Number of components below the drive root: `c:\a` -> 1, `c:\a\b` -> 2.
pub fn depth_from_root(norm: &str) -> usize {
    norm.trim_end_matches('\\').matches('\\').count()
}

/// Last path component (works on either separator).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or(path)
}

/// Parent path, or None at a root.
pub fn parent_of(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let idx = trimmed.rfind(['\\', '/'])?;
    if idx == 0 {
        return Some(&trimmed[..1]);
    }
    let p = &trimmed[..idx];
    // "C:" -> "C:\"
    if p.len() == 2 && p.ends_with(':') {
        return Some(&trimmed[..idx + 1]);
    }
    Some(p)
}

pub fn extension(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot == name.len() - 1 {
        return None;
    }
    Some(name[dot + 1..].to_ascii_lowercase())
}

pub fn unix_secs(t: std::io::Result<SystemTime>) -> i64 {
    t.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_secs() -> i64 {
    unix_secs(Ok(SystemTime::now()))
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Unix seconds -> "YYYY-MM-DD" (UTC). Avoids pulling in a date crate.
pub fn ymd(secs: i64) -> String {
    if secs <= 0 {
        return "unknown".into();
    }
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_within() {
        assert_eq!(normalize("C:/Users/Bob/"), "c:\\users\\bob");
        assert_eq!(normalize("C:\\"), "c:\\");
        assert!(is_within("c:\\windows\\temp", "c:\\windows"));
        assert!(!is_within("c:\\windowsapps", "c:\\windows"));
        assert!(is_within("c:\\foo", "c:\\"));
    }

    #[test]
    fn parents() {
        assert_eq!(parent_of("C:\\a\\b"), Some("C:\\a"));
        assert_eq!(parent_of("C:\\a"), Some("C:\\"));
        assert_eq!(parent_of("/tmp/x"), Some("/tmp"));
        assert_eq!(parent_of("/tmp"), Some("/"));
    }

    #[test]
    fn dates_and_sizes() {
        assert_eq!(ymd(0), "unknown");
        assert_eq!(ymd(1_700_000_000), "2023-11-14");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(extension("Setup.MSI"), Some("msi".into()));
        assert_eq!(extension(".gitignore"), None);
    }
}
