//! Uninstalling a game found in a library folder: deleting its folder, and nothing else. Its
//! Wine prefix (the saves) is left alone, and so is anything a link inside the folder points to.
//!
//! Games added by hand (an `.exe` picked with Add game) are never uninstalled: their folder is
//! just wherever the `.exe` was, which could be ~/Downloads. They're only removed from the
//! library.

use crate::paths::Paths;
use std::path::{Path, PathBuf};

/// Why the folder `root` mustn't be deleted, if it mustn't. It has to be a real folder (not a
/// link) directly inside one of `libraries`, not a library itself or one's parent, not holding
/// asterofix's own files or the home folder, not holding the game's Wine `prefix`, and it must
/// still look like a game. Checked when Uninstall is pressed, and again just before deleting.
pub fn check(paths: &Paths, libraries: &[PathBuf], root: &Path, prefix: &Path) -> Result<(), String> {
    let shown = root.display();
    if !root.is_absolute() {
        return Err(format!("{shown} isn't an absolute path"));
    }
    let meta = std::fs::symlink_metadata(root).map_err(|e| format!("{shown}: {e}"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{shown} is a link, not the game's folder"));
    }
    if !meta.is_dir() {
        return Err(format!("{shown} isn't a folder"));
    }
    if !root.parent().is_some_and(|p| libraries.iter().any(|l| l == p)) {
        return Err(format!("{shown} isn't in a library folder"));
    }
    if libraries.iter().any(|l| l.starts_with(root)) {
        return Err(format!("{shown} is a library folder"));
    }
    let ours = [paths.home.clone(), paths.data_dir(), paths.config_dir(), paths.cache_dir()];
    if ours.iter().any(|p| p.starts_with(root)) {
        return Err(format!("{shown} holds asterofix's own files"));
    }
    if prefix.starts_with(root) {
        return Err(format!("{shown} holds the game's Wine prefix, with its saves"));
    }
    if crate::scan::detect_game(root).is_none() {
        return Err(format!("{shown} doesn't hold a game any more"));
    }
    Ok(())
}

/// Deletes the folder `root` and everything in it. Links inside it are removed, never followed.
pub fn delete(root: &Path) -> std::io::Result<()> {
    // std's remove_dir_all opens each folder without following links (O_NOFOLLOW), so a link
    // to somewhere else is unlinked, and what it points to is untouched.
    std::fs::remove_dir_all(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{add_game, tempdir, write};

    #[test]
    fn only_a_game_directly_in_a_library_can_be_uninstalled() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let lib = home.path().join("Games");
        let inner = lib.join("asterofix");
        let libs = [lib.clone(), inner.clone()];
        let prefix = paths.data_dir().join("prefixes/Pong");
        let pong = add_game(&lib, "Pong", 0);
        let check = |root: &Path| check(&paths, &libs, root, &prefix);
        assert_eq!(check(&pong), Ok(()));

        // Deeper than a library's own games.
        let deep = add_game(&pong, "Mod", 0);
        assert!(check(&deep).unwrap_err().contains("isn't in a library"));
        // A game's folder that's another library, or holds one.
        add_game(&inner, "Hatch", 0);
        write(&inner.join("SteamFix64.dll"), "x");
        assert!(check(&inner).unwrap_err().contains("is a library"));
        let libs = [lib.clone(), lib.join("Pong/Sub")];
        assert!(super::check(&paths, &libs, &pong, &prefix).unwrap_err().contains("is a library"));
        // Not there, relative, or not a game any more.
        assert!(check(&lib.join("Gone")).is_err());
        assert!(check(Path::new("Games/Pong")).unwrap_err().contains("absolute"));
        let plain = lib.join("Photos");
        write(&plain.join("cat.jpg"), "x");
        assert!(check(&plain).unwrap_err().contains("doesn't hold a game"));
    }

    #[test]
    fn the_saves_and_asterofixs_files_are_never_in_what_is_deleted() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let lib = home.path().join("Games");
        let pong = add_game(&lib, "Pong", 0);
        // A prefix picked by hand inside the game's folder.
        let err = check(&paths, std::slice::from_ref(&lib), &pong, &pong.join("pfx")).unwrap_err();
        assert!(err.contains("Wine prefix"), "{err}");
        // A library of home folders: the home folder itself is a "game" in it.
        add_game(home.path().parent().unwrap(), &crate::scan::file_name(home.path()), 0);
        let libs = [home.path().parent().unwrap().to_path_buf()];
        let err = check(&paths, &libs, home.path(), Path::new("/elsewhere")).unwrap_err();
        assert!(err.contains("asterofix's own files"), "{err}");
    }

    #[test]
    fn a_linked_game_folder_is_refused() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let lib = home.path().join("Games");
        let real = add_game(home.path(), "Pong", 0);
        std::fs::create_dir_all(&lib).unwrap();
        std::os::unix::fs::symlink(&real, lib.join("Pong")).unwrap();
        let err = check(&paths, std::slice::from_ref(&lib), &lib.join("Pong"), Path::new("/p")).unwrap_err();
        assert!(err.contains("is a link"), "{err}");
    }

    #[test]
    fn deleting_never_follows_links_out_of_the_folder() {
        let home = tempdir();
        let pong = add_game(&home.path().join("Games"), "Pong", 0);
        let outside = home.path().join("Documents");
        write(&outside.join("keep.txt"), "mine");
        std::os::unix::fs::symlink(&outside, pong.join("docs")).unwrap();
        std::os::unix::fs::symlink(outside.join("keep.txt"), pong.join("keep.txt")).unwrap();
        delete(&pong).unwrap();
        assert!(!pong.exists());
        assert_eq!(std::fs::read_to_string(outside.join("keep.txt")).unwrap(), "mine");
    }
}
