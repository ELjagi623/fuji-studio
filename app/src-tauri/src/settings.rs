//! Persistent app settings (config dir `settings.json`) and the in-app folder browser.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Settings {
    /// Folder the select page was showing last time (restored on start).
    pub last_dir: Option<String>,
    /// "包含子文件夹" toggle on the select page.
    pub include_subfolders: bool,
    /// select-page view: "xl" | "large" | "medium" | "small" | "list" ("" = large)
    pub view: String,
    /// "name" | "date" | "size" | "model" ("" = name)
    pub sort_by: String,
    pub sort_desc: bool,
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        // unknown keys from older versions (card cache settings) are ignored
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub has_children: bool,
    /// "place" (Pictures, Desktop…), "drive" or "dir"
    pub kind: &'static str,
}

fn hidden(entry: &std::fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if name.starts_with('.') || name.starts_with('$') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if let Ok(m) = entry.metadata() {
            if m.file_attributes() & (HIDDEN | SYSTEM) != 0 {
                return true;
            }
        }
    }
    // Finder hides folders flagged UF_HIDDEN (e.g. ~/Library) even without a leading dot.
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        const UF_HIDDEN: u32 = 0x8000;
        if let Ok(m) = entry.metadata() {
            if m.st_flags() & UF_HIDDEN != 0 {
                return true;
            }
        }
    }
    false
}

fn visible_subdirs(dir: &Path) -> impl Iterator<Item = std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false) && !hidden(e))
}

fn has_subdirs(dir: &Path) -> bool {
    visible_subdirs(dir).next().is_some()
}

/// Sub-folders of `dir`, sorted by name (case-insensitive), hidden/system ones skipped.
pub fn list_dirs(dir: &Path) -> Result<Vec<DirNode>> {
    std::fs::read_dir(dir)?; // surface "access denied" etc. as an error
    let mut out: Vec<DirNode> = visible_subdirs(dir)
        .map(|e| {
            let p = e.path();
            DirNode {
                name: e.file_name().to_string_lossy().into(),
                has_children: has_subdirs(&p),
                path: p.to_string_lossy().into(),
                kind: "dir",
            }
        })
        .collect();
    out.sort_by_key(|n| n.name.to_lowercase());
    Ok(out)
}

/// Quick places + all drives, shown at the top level of the folder tree.
pub fn roots(places: &[(&str, Option<PathBuf>)]) -> Vec<DirNode> {
    let mut out = Vec::new();
    for (label, p) in places {
        if let Some(p) = p.as_ref().filter(|p| p.is_dir()) {
            out.push(DirNode { name: label.to_string(), path: p.to_string_lossy().into(), has_children: has_subdirs(p), kind: "place" });
        }
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        let p = Path::new(&root);
        if std::fs::read_dir(p).is_ok() {
            out.push(DirNode { name: format!("{}:", letter as char), path: root.clone(), has_children: has_subdirs(p), kind: "drive" });
        }
    }
    // macOS: the startup disk, then everything mounted under /Volumes (external drives,
    // SD card readers, network shares), like the Finder sidebar's "Locations".
    #[cfg(target_os = "macos")]
    {
        out.push(DirNode { name: "Macintosh HD".into(), path: "/".into(), has_children: true, kind: "drive" });
        let mut vols: Vec<_> = std::fs::read_dir("/Volumes")
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                // /Volumes/<startup disk> is a symlink back to "/": already listed above
                !e.file_type().map(|t| t.is_symlink()).unwrap_or(false) && e.path().is_dir() && !hidden(e)
            })
            .collect();
        vols.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
        for e in vols {
            let p = e.path();
            out.push(DirNode { name: e.file_name().to_string_lossy().into(), has_children: has_subdirs(&p), path: p.to_string_lossy().into(), kind: "drive" });
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    out.push(DirNode { name: "/".into(), path: "/".into(), has_children: true, kind: "drive" });
    out
}

/// "YYYY-MM-DD" and "YYYY-MM-DD HH:MM" (UTC) for a unix timestamp.
pub fn unix_date(secs: u64) -> (String, String) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let day = format!("{y:04}-{m:02}-{d:02}");
    (day.clone(), format!("{day} {:02}:{:02}", rem / 3600, (rem % 3600) / 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(unix_date(0).1, "1970-01-01 00:00");
        assert_eq!(unix_date(1_700_000_000).1, "2023-11-14 22:13");
    }

    #[test]
    fn lists_sorted_skips_hidden() {
        let t = std::env::temp_dir().join(format!("fs-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        for d in ["b", "A/inner", ".hidden", "c"] {
            std::fs::create_dir_all(t.join(d)).unwrap();
        }
        std::fs::write(t.join("file.RAF"), b"x").unwrap();
        let l = list_dirs(&t).unwrap();
        let names: Vec<_> = l.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["A", "b", "c"]);
        assert!(l[0].has_children && !l[1].has_children);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_roots_and_hidden_folders() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let r = roots(&[("主目录", Some(home.clone()))]);
        assert!(r.iter().any(|n| n.kind == "place" && n.path == home.to_string_lossy()));
        // the startup disk appears exactly once (the /Volumes/<name> symlink back to "/" is skipped)
        assert_eq!(r.iter().filter(|n| n.path == "/").count(), 1, "{r:?}");
        assert!(r.iter().any(|n| n.name == "Macintosh HD" && n.kind == "drive"));
        // ~/Library carries the Finder "hidden" flag
        let names: Vec<_> = list_dirs(&home).unwrap().into_iter().map(|n| n.name).collect();
        assert!(!names.contains(&"Library".to_string()), "{names:?}");
    }

    #[test]
    fn old_settings_file_still_loads() {
        let t = std::env::temp_dir().join(format!("fs-settings-{}.json", std::process::id()));
        std::fs::write(&t, r#"{"card_cache_dir":null,"card_cache_limit_gb":20.0,"last_dir":"D:\\x"}"#).unwrap();
        let s = Settings::load(&t);
        assert_eq!(s.last_dir.as_deref(), Some("D:\\x"));
        std::fs::remove_file(&t).unwrap();
    }
}
