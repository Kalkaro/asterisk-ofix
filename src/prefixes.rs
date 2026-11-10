//! The Wine prefixes in `~/.local/share/asterofix/prefixes/`, which hold the games' saves, as
//! the Prefix field's menu lists them. Nothing here moves, renames or deletes a prefix; a game
//! is only pointed at one.

use crate::launch::prefix_owner;
use crate::paths::Paths;
use crate::scan::file_name;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Prefix {
    pub path: PathBuf,
    /// Its folder's name.
    pub name: String,
    /// The game folder that launched in it first (`claim_default_prefix`), if it says.
    pub owner: Option<PathBuf>,
    /// When a game last ran in it, as near as its files tell.
    pub used: SystemTime,
}

impl Prefix {
    /// Its owner's folder is gone: the game was renamed, moved or deleted.
    pub fn orphaned(&self) -> bool {
        self.owner.as_deref().is_some_and(|o| !o.exists())
    }
}

/// Where the prefixes are.
pub fn prefixes_dir(paths: &Paths) -> PathBuf {
    paths.data_dir().join("prefixes")
}

/// Every prefix, by name.
pub fn list(paths: &Paths) -> Vec<Prefix> {
    let mut out: Vec<Prefix> = crate::scan::read_dir_sorted(&prefixes_dir(paths))
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|path| Prefix { name: file_name(&path), owner: prefix_owner(&path), used: last_used(&path), path })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// Wine rewrites its registry files as a prefix's game exits, and Proton touches its lock and
/// version files on every launch.
fn last_used(prefix: &Path) -> SystemTime {
    ["", "pfx", "pfx/user.reg", "pfx/system.reg", "pfx.lock", "version"]
        .iter()
        .filter_map(|rel| std::fs::metadata(prefix.join(rel)).and_then(|m| m.modified()).ok())
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// A prefix Proton has run in, last at `secs` past the epoch, owned by `owner` if given.
#[cfg(test)]
pub fn fake_prefix(paths: &Paths, name: &str, owner: Option<&Path>, secs: u64) -> PathBuf {
    let dir = prefixes_dir(paths).join(name);
    crate::testutil::write(&dir.join("pfx/user.reg"), "WINE REGISTRY");
    if let Some(owner) = owner {
        crate::testutil::write(&dir.join(".asterofix-game"), owner.to_string_lossy().as_bytes());
    }
    let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    for p in [dir.join("pfx/user.reg"), dir.join("pfx"), dir.clone()] {
        std::fs::File::open(&p).unwrap().set_modified(at).unwrap();
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{tempdir, write};
    use std::time::Duration;

    #[test]
    fn lists_prefixes_with_their_owners() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        assert!(list(&paths).is_empty(), "no prefixes folder yet");
        let pong = home.path().join("Games/Pong");
        std::fs::create_dir_all(&pong).unwrap();
        fake_prefix(&paths, "Pong", Some(&pong), 100);
        fake_prefix(&paths, "gone", Some(&home.path().join("Games/Gone")), 300);
        fake_prefix(&paths, "Bare", None, 200);
        write(&prefixes_dir(&paths).join("stray.txt"), "not a prefix");

        let all = list(&paths);
        assert_eq!(all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Bare", "gone", "Pong"]);
        assert_eq!(all[2].owner.as_deref(), Some(pong.as_path()));
        assert!(!all[2].orphaned());
        assert!(all[1].orphaned(), "its owner's folder is gone");
        assert!(all[0].owner.is_none() && !all[0].orphaned());
        assert_eq!(all[1].used, SystemTime::UNIX_EPOCH + Duration::from_secs(300));
    }
}
