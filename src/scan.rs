//! Finding OnlineFix / SteamFix games in library folders.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Files dropped next to the game executable by OnlineFix (old and new style).
const MARKERS: &[&str] =
    &["onlinefix.ini", "onlinefix64.dll", "onlinefix.dll", "steamfix.ini", "steamfix64.dll", "steamfix.dll"];

/// DLLs that must be loaded from the game folder instead of Wine's builtins.
const NATIVE_DLLS: &[&str] = &[
    "OnlineFix64",
    "OnlineFix",
    "SteamFix64",
    "SteamFix",
    "SteamOverlay64",
    "SteamOverlay",
    "steam_api64",
    "steam_api",
    "dnet",
];

/// Proxy DLLs OnlineFix injects itself through: native first, builtin fallback.
const PROXY_DLLS: &[&str] = &["winmm", "winhttp", "version"];

/// The string from the usual OnlineFix-on-Linux guides.
pub const FALLBACK_OVERRIDES: &str = "OnlineFix64=n;SteamOverlay64=n;winmm=n,b;dnet=n;steam_api64=n;winhttp=n,b";

const EXE_BLOCKLIST: &[&str] = &[
    "unins",
    "unitycrashhandler",
    "crashreport",
    "crashhandler",
    "crashpad",
    "setup",
    "redist",
    "dxwebsetup",
    "dotnet",
    "vcredist",
    "vc_redist",
    "ue4prereq",
    "easyanticheat",
    "notification_helper",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Game {
    pub name: String,
    /// The top-level game folder, also the key its settings are saved under.
    pub root: PathBuf,
    /// The folder holding the OnlineFix files.
    pub fix_dir: PathBuf,
    pub exes: Vec<PathBuf>,
    pub real_id: u32,
    pub fake_id: u32,
}

pub fn scan_libraries(libs: &[PathBuf]) -> Vec<Game> {
    let mut games = Vec::new();
    let mut seen = HashSet::new();
    for lib in libs {
        for p in read_dir_sorted(lib) {
            // A library inside another (~/Games/asterisk in ~/Games) is scanned as one, not
            // taken for a game.
            if !p.is_dir() || libs.contains(&p) {
                continue;
            }
            if let Some(g) = detect_game(&p)
                && seen.insert(g.fix_dir.clone())
            {
                games.push(g);
            }
        }
    }
    games
}

/// Looks for OnlineFix files in `root` or a few levels below it.
pub fn detect_game(root: &Path) -> Option<Game> {
    let fix_dir = find_fix_dir(root, 3)?;
    let mut exes = exe_candidates(&fix_dir);
    if exes.is_empty() && fix_dir != root {
        exes = exe_candidates(root);
    }
    let folder = file_name(root);
    rank_exes(&mut exes, &folder);
    let (real_id, fake_id) = read_fix_ini(&fix_dir);
    Some(Game { name: pretty_name(&folder), root: root.to_path_buf(), fix_dir, exes, real_id, fake_id })
}

/// An entry for a hand-picked executable.
pub fn game_from_exe(exe: &Path) -> Game {
    let dir = exe.parent().unwrap_or(Path::new("/"));
    let mut g = detect_game(dir).unwrap_or_else(|| Game {
        root: dir.to_path_buf(),
        fix_dir: dir.to_path_buf(),
        ..Game::default()
    });
    let mut exes = vec![exe.to_path_buf()];
    exes.extend(g.exes.into_iter().filter(|e| e != exe));
    g.exes = exes;
    g.name = pretty_name(&exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    g
}

fn find_fix_dir(dir: &Path, depth: u32) -> Option<PathBuf> {
    let entries = read_dir_sorted(dir);
    if entries.iter().any(|p| MARKERS.contains(&file_name(p).to_lowercase().as_str()) && !p.is_dir()) {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    entries.iter().filter(|p| p.is_dir()).find_map(|p| find_fix_dir(p, depth - 1))
}

fn exe_candidates(dir: &Path) -> Vec<PathBuf> {
    read_dir_sorted(dir)
        .into_iter()
        .filter(|p| !p.is_dir() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
        .filter(|p| {
            let stem = stem(p).to_lowercase();
            !EXE_BLOCKLIST.iter().any(|b| stem.contains(b))
        })
        .collect()
}

/// Puts names resembling the folder first, then larger files.
fn rank_exes(exes: &mut [PathBuf], folder: &str) {
    let f = simplify(folder);
    exes.sort_by_cached_key(|p| {
        let s = simplify(&stem(p));
        let resembles = !s.is_empty() && (f.contains(&s) || s.contains(&f));
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        (!resembles, std::cmp::Reverse(size))
    });
}

/// RealAppId / FakeAppId from OnlineFix.ini or SteamFix.ini.
fn read_fix_ini(dir: &Path) -> (u32, u32) {
    let Some(ini) = read_dir_sorted(dir).into_iter().find(|p| {
        let n = file_name(p).to_lowercase();
        n == "onlinefix.ini" || n == "steamfix.ini"
    }) else {
        return (0, 0);
    };
    let Ok(data) = std::fs::read(&ini) else { return (0, 0) };
    let (mut real, mut fake, mut linux_real) = (0, 0, 0);
    let mut section = String::new();
    for line in String::from_utf8_lossy(&data).lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_lowercase();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let Ok(n) = v.trim().parse::<u32>() else { continue };
        match (section.as_str(), k.trim().to_lowercase().as_str()) {
            ("main", "realappid") => real = n,
            ("main", "fakeappid") => fake = n,
            // Written by an older third-party OnlineFix launcher.
            ("onlinefix linux", "realappid") => linux_real = n,
            _ => {}
        }
    }
    if linux_real != 0 {
        real = linux_real;
    }
    (real, fake)
}

/// WINEDLLOVERRIDES for the DLLs next to the executable.
pub fn dll_overrides(exe_dir: &Path) -> String {
    let present: HashSet<String> = read_dir_sorted(exe_dir)
        .iter()
        .filter_map(|p| file_name(p).to_lowercase().strip_suffix(".dll").map(str::to_owned))
        .collect();
    let mut parts: Vec<String> =
        NATIVE_DLLS.iter().filter(|d| present.contains(&d.to_lowercase())).map(|d| format!("{d}=n")).collect();
    if parts.is_empty() {
        return FALLBACK_OVERRIDES.into();
    }
    // Unity and Unreal keep the patched steam_api64.dll in a plugins subfolder.
    if !present.contains("steam_api64") {
        parts.push("steam_api64=n".into());
    }
    parts.extend(PROXY_DLLS.iter().filter(|d| present.contains(**d)).map(|d| format!("{d}=n,b")));
    parts.join(";")
}

fn simplify(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || !c.is_ascii()).collect()
}

pub fn pretty_name(folder: &str) -> String {
    folder.replace(['.', '_'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
    out.sort();
    out
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{tempdir, write};

    #[test]
    fn detects_a_steamfix_game() {
        let lib = tempdir();
        let game = lib.path().join("Cool.Game");
        for f in ["Cool Game.exe", "UnityCrashHandler64.exe", "unins000.exe", "SteamFix64.dll", "winmm.dll"] {
            write(&game.join(f), "x");
        }
        write(
            &game.join("SteamFix.ini"),
            "[Main]\nRealAppId=480\nFakeAppId=480\n[OnlineFix Linux]\nRealAppId=1000002\n",
        );

        let games = scan_libraries(&[lib.path().to_path_buf()]);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!((g.name.as_str(), g.real_id, g.fake_id), ("Cool Game", 1000002, 480));
        assert_eq!(g.exes, vec![game.join("Cool Game.exe")]);
        assert_eq!(dll_overrides(&game), "SteamFix64=n;steam_api64=n;winmm=n,b");
        assert_eq!(dll_overrides(tempdir().path()), FALLBACK_OVERRIDES);
    }

    #[test]
    fn a_library_inside_another_holds_games_rather_than_being_one() {
        let lib = tempdir();
        let inner = lib.path().join("asterisk");
        crate::testutil::add_game(&inner, "Pong", 0);
        crate::testutil::add_game(lib.path(), "Hatch", 0);
        let names = |libs: &[PathBuf]| scan_libraries(libs).into_iter().map(|g| g.name).collect::<Vec<_>>();
        assert_eq!(names(&[lib.path().to_path_buf(), inner.clone()]), ["Hatch", "Pong"]);
        assert_eq!(names(&[inner, lib.path().to_path_buf()]), ["Pong", "Hatch"]);
    }

    #[test]
    fn finds_fix_files_in_a_subfolder() {
        let lib = tempdir();
        let root = lib.path().join("Deep_Game");
        write(&root.join("Bin/Win64/Deep.exe"), "x");
        write(&root.join("Bin/Win64/OnlineFix64.dll"), "x");
        let g = detect_game(&root).unwrap();
        assert_eq!(g.fix_dir, root.join("Bin/Win64"));
        assert_eq!(g.exes, vec![root.join("Bin/Win64/Deep.exe")]);
        assert!(detect_game(tempdir().path()).is_none());
    }

    #[test]
    fn ranks_the_exe_named_like_the_folder_first() {
        let lib = tempdir();
        let root = lib.path().join("Pong");
        write(&root.join("Big Launcher.exe"), vec![0u8; 5000]);
        write(&root.join("Pong.exe"), "x");
        write(&root.join("SteamFix64.dll"), "x");
        assert_eq!(detect_game(&root).unwrap().exes[0], root.join("Pong.exe"));
    }

    #[test]
    fn a_hand_picked_exe_comes_first() {
        let lib = tempdir();
        let root = crate::testutil::add_game(lib.path(), "Hatch", 0);
        write(&root.join("Other_Tool.exe"), "x");
        let g = game_from_exe(&root.join("Other_Tool.exe"));
        assert_eq!(g.exes[0], root.join("Other_Tool.exe"));
        assert_eq!(g.name, "Other Tool");
        assert_eq!(g.root, root);
    }
}
