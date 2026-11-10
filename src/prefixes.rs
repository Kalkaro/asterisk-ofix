//! The Wine prefixes in `~/.local/share/asterofix/prefixes/`, which hold the games' saves: what
//! the Prefix field's menu lists, and the old prefix offered to a game whose folder was renamed
//! (a repack updated from "Game v1.0" to "Game v1.1" starts in a fresh prefix otherwise).
//! Nothing here moves, renames or deletes a prefix; a game is only pointed at one.

use crate::launch::{default_prefix, prefix_owner};
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

/// The prefixes that may hold the saves of the game in `root` from before its folder was
/// renamed or moved, most recently used first. Only for a game that has never run in its own
/// prefix, and kept strict, since a wrong match points one game at another's saves:
///
/// - a prefix whose owner folder still exists is that game's, never offered;
/// - nor is one in `taken`: another game's in the library, its own or one it was pointed at;
/// - the owner's folder name, without versions and repack tags, must name the same game as
///   `root`'s, whatever the case (`install::same_game`);
/// - a prefix with no owner file to say which folder it was only matches when its own name
///   reduces to exactly what the game's folder name does, case and all;
/// - and it has to be a prefix Proton made (`pfx`), not an empty folder.
pub fn old_saves<'a>(paths: &Paths, prefixes: &'a [Prefix], root: &Path, taken: &[PathBuf]) -> Vec<&'a Prefix> {
    let own = default_prefix(paths, root);
    if own.exists() {
        return Vec::new();
    }
    let folder = file_name(root);
    let mut found: Vec<&Prefix> = prefixes
        .iter()
        .filter(|p| p.path != own && !taken.contains(&p.path) && p.path.join("pfx").is_dir())
        .filter(|p| match &p.owner {
            Some(owner) => !owner.exists() && crate::install::same_game(&file_name(owner), &folder),
            None => crate::install::same_game_exactly(&p.name, &crate::launch::slug(&folder)),
        })
        .collect();
    found.sort_by_key(|p| std::cmp::Reverse(p.used));
    found
}

/// What old saves are called: the folder that owned them, or the prefix's own name.
pub fn old_name(p: &Prefix) -> String {
    p.owner.as_deref().map(file_name).unwrap_or_else(|| p.name.clone())
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

    fn names(found: Vec<&Prefix>) -> Vec<&str> {
        found.into_iter().map(|p| p.name.as_str()).collect()
    }

    /// "Game v1.0" updated to "Game v1.1" started in a fresh prefix, and the saves looked lost.
    #[test]
    fn finds_the_saves_of_a_renamed_folder() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let games = home.path().join("Games");
        let new = games.join("Lucid Cats v1.1 by Pioneer");
        std::fs::create_dir_all(&new).unwrap();
        fake_prefix(&paths, "Lucid_Cats_v1.0", Some(&games.join("Lucid Cats v1.0")), 100);
        let all = list(&paths);
        assert_eq!(names(old_saves(&paths, &all, &new, &[])), ["Lucid_Cats_v1.0"]);
        assert_eq!(old_name(old_saves(&paths, &all, &new, &[])[0]), "Lucid Cats v1.0");

        // Not once the game has a prefix of its own: it ran there, and has saves of its own.
        fake_prefix(&paths, "Lucid_Cats_v1.1_by_Pioneer", None, 200);
        assert!(old_saves(&paths, &list(&paths), &new, &[]).is_empty());
    }

    #[test]
    fn never_offers_another_games_saves() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let games = home.path().join("Games");
        let game = games.join("Pong v2");
        let live = games.join("Pong v1");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(&live).unwrap();
        // Its owner is still there.
        fake_prefix(&paths, "Pong_v1", Some(&live), 100);
        // Another game in the library uses it, whatever its owner file says.
        let taken = fake_prefix(&paths, "Pong_v0", Some(&games.join("Pong v0")), 100);
        // Another game altogether.
        fake_prefix(&paths, "Pong_Deluxe_v1", Some(&games.join("Pong Deluxe v1")), 100);
        // Not a prefix Proton made.
        let empty = prefixes_dir(&paths).join("Pong_v0.9");
        write(&empty.join(".asterofix-game"), games.join("Pong v0.9").to_string_lossy().as_bytes());
        let all = list(&paths);
        let found = names(old_saves(&paths, &all, &game, &[taken]));
        assert!(found.is_empty(), "{found:?}");
        assert_eq!(names(old_saves(&paths, &all, &game, &[])), ["Pong_v0"], "only because nothing uses it");

        // A number alone is part of the name.
        fake_prefix(&paths, "Game_2", Some(&games.join("Game 2")), 100);
        assert!(old_saves(&paths, &list(&paths), &games.join("Game 3"), &[]).is_empty());
    }

    /// With no owner file to say which folder it was, a prefix's name has to reduce to exactly
    /// the game's.
    #[test]
    fn a_prefix_without_an_owner_needs_an_exact_name() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let game = home.path().join("Games/Hollow Pines v1.2");
        std::fs::create_dir_all(&game).unwrap();
        fake_prefix(&paths, "hollow_pines", None, 100);
        fake_prefix(&paths, "Hollow_Pines_Demo", None, 100);
        assert!(old_saves(&paths, &list(&paths), &game, &[]).is_empty());
        fake_prefix(&paths, "Hollow_Pines_v1.0", None, 100);
        assert_eq!(names(old_saves(&paths, &list(&paths), &game, &[])), ["Hollow_Pines_v1.0"]);

        // An owner's name matches whatever its case.
        let other = home.path().join("Games/Saltwind 2.0");
        std::fs::create_dir_all(&other).unwrap();
        fake_prefix(&paths, "SALTWIND_v1", Some(&home.path().join("Games/SALTWIND v1")), 100);
        assert_eq!(names(old_saves(&paths, &list(&paths), &other, &[])), ["SALTWIND_v1"]);
    }

    #[test]
    fn the_most_recently_played_comes_first() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let games = home.path().join("Games");
        let game = games.join("Pong v3");
        std::fs::create_dir_all(&game).unwrap();
        fake_prefix(&paths, "Pong_v1", Some(&games.join("Pong v1")), 100);
        fake_prefix(&paths, "Pong_v2", Some(&games.join("Pong v2")), 300);
        fake_prefix(&paths, "Pong", None, 200);
        assert_eq!(names(old_saves(&paths, &list(&paths), &game, &[])), ["Pong_v2", "Pong", "Pong_v1"]);
    }
}
