//! Installing a game from its Windows installer (a repack's setup.exe) and then its online fix,
//! silently, into `~/Games/asterofix`. Each installer runs through Proton like a game does, in
//! the prefix the game will play in.

use crate::launch::{LaunchSpec, default_prefix, shell_quote};
use crate::paths::Paths;
use crate::scan::{file_name, pretty_name, read_dir_sorted};
use crate::steam::Proton;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where installed games go, under the home folder. It's added to the libraries.
pub const INSTALL_DIR: &str = "Games/asterofix";

/// What the installers' names say: a game's setup and an online fix differ only in this.
const FIX_WORDS: &[&str] = &["fix", "onlinefix", "steamfix", "multiplayer", "online", "coop", "co-op", "repair"];

/// Setup programs named for what they are rather than the game; their folder names the game.
const GENERIC_STEMS: &[&str] = &["setup", "install", "installer", "autorun", "launcher", "start"];

/// How long the start of an installer is searched for its kind. Inno Setup's marker sits in its
/// loader, NSIS's right after its stub; a repack's data can be gigabytes after them.
const SNIFF_BYTES: u64 = 4 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Inno,
    Nsis,
}

/// One installer to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub exe: PathBuf,
    pub kind: Kind,
    pub is_fix: bool,
}

/// A game to install: where it goes, and its installers in the order they run, the game's
/// first and its fixes after.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub name: String,
    pub dest: PathBuf,
    pub steps: Vec<Step>,
}

/// Sorts the chosen installers into the game's and its fixes, whatever order they were
/// picked in. Without the game's, the fixes go into that game's folder, which must exist.
pub fn plan(paths: &Paths, files: &[PathBuf]) -> Result<Plan, String> {
    let mut steps = Vec::new();
    for exe in files {
        let kind = kind(exe)?;
        steps.push(Step { exe: exe.clone(), kind, is_fix: looks_like_fix(exe) });
    }
    // Stable: fixes keep the order they were picked in.
    steps.sort_by_key(|s| s.is_fix);
    let games: Vec<&Step> = steps.iter().filter(|s| !s.is_fix).collect();
    let first = match games.as_slice() {
        [] => steps.first().ok_or("no installer chosen")?,
        [game] => *game,
        _ => return Err("pick one game's installer at a time".into()),
    };
    let name = game_name(&first.exe);
    if name.is_empty() {
        return Err(format!("can't tell the game's name from {}", first.exe.display()));
    }
    let dest = paths.home.join(INSTALL_DIR).join(&name);
    let installs_game = !first.is_fix;
    if installs_game && dir_has_files(&dest) {
        return Err(format!("{} already exists", dest.display()));
    }
    if !installs_game && !dest.is_dir() {
        return Err(format!("install {name} first: {} doesn't exist", dest.display()));
    }
    Ok(Plan { name, dest, steps })
}

impl Plan {
    /// The launch that runs step `i` silently. Its log is the game's, named for the step, so
    /// installing never overwrites the log of a launch.
    pub fn spec(&self, paths: &Paths, i: usize, proton: &Proton) -> LaunchSpec {
        let step = &self.steps[i];
        let suffix = if step.is_fix { "fix" } else { "setup" };
        LaunchSpec {
            name: format!("{} {suffix}", self.name),
            exe: step.exe.clone(),
            game_dir: self.dest.clone(),
            proton: proton.clone(),
            prefix: default_prefix(paths, &self.dest),
            fake_app_id: 480,
            // Installers open their readme or a website when they finish, even silently.
            overrides: "winebrowser.exe=d".into(),
            env: String::new(),
            args: silent_args(step.kind, &self.dest).iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" "),
        }
    }

    /// Checks what step `i` left behind once it exits.
    pub fn check(&self, i: usize) -> Result<(), String> {
        if !self.steps[i].is_fix && !dir_has_files(&self.dest) {
            return Err(format!("the installer finished without installing anything in {}", self.dest.display()));
        }
        Ok(())
    }
}

/// What the installer is, from the marker its kind leaves near the start of the file.
pub fn kind(exe: &Path) -> Result<Kind, String> {
    let mut head = Vec::new();
    std::fs::File::open(exe)
        .and_then(|f| f.take(SNIFF_BYTES).read_to_end(&mut head))
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let has = |marker: &[u8]| head.windows(marker.len()).any(|w| w == marker);
    if has(b"Inno Setup Setup Data") {
        Ok(Kind::Inno)
    } else if has(b"NullsoftInst") {
        Ok(Kind::Nsis)
    } else {
        Err(format!("{} isn't an Inno Setup or NSIS installer, so it can't run silently", file_name(exe)))
    }
}

/// The arguments that run an installer with no window and no questions, into `dest`.
pub fn silent_args(kind: Kind, dest: &Path) -> Vec<String> {
    let dir = windows_path(dest);
    match kind {
        Kind::Inno => vec![
            "/VERYSILENT".into(),
            "/SUPPRESSMSGBOXES".into(),
            "/NORESTART".into(),
            "/SP-".into(),
            "/NOICONS".into(),
            format!("/DIR={dir}"),
        ],
        // NSIS takes the rest of the line as the folder, unquoted, so /D comes last.
        Kind::Nsis => vec!["/S".into(), format!("/D={dir}")],
    }
}

/// `path` as Wine sees it, on the Z: drive that maps the root.
fn windows_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

/// The online fix downloaded for the game `setup` installs: beside it, or beside its folder
/// (a repack's folder and the fix next to it in ~/Downloads).
pub fn find_fix(setup: &Path) -> Option<PathBuf> {
    let name = game_name(setup).to_lowercase();
    let dirs = [setup.parent(), setup.parent().and_then(Path::parent)];
    dirs.into_iter().flatten().flat_map(read_dir_sorted).find(|p| {
        p != setup
            && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            && looks_like_fix(p)
            && game_name(p).to_lowercase() == name
    })
}

/// Whether an installer's name says it's an online fix rather than a game.
pub fn looks_like_fix(exe: &Path) -> bool {
    words(&stem(exe)).iter().any(|w| matches!(w.to_lowercase().as_str(), "fix" | "onlinefix" | "steamfix"))
}

/// The game an installer installs, as its folder should be called: "Lucid Cats" from
/// `Lucid Cats v2.4 by Pioneer/setup.exe` or `Lucid-Cats-Multiplayer-Fix-Online.exe`.
pub fn game_name(exe: &Path) -> String {
    let stem = stem(exe);
    let generic = {
        let s = stem.to_lowercase();
        GENERIC_STEMS.iter().any(|g| s == *g || s.starts_with(&format!("{g}_")) || s.starts_with(&format!("{g}-")))
    };
    let folder = exe.parent().map(file_name).unwrap_or_default();
    let name = if generic { String::new() } else { clean_name(&stem) };
    // "OnlineFix.exe" names nothing either.
    if name.is_empty() { clean_name(&folder) } else { name }
}

/// A file name without its version, repacker and fix words.
fn clean_name(source: &str) -> String {
    let mut words = words(source);
    // A repacker's name: "... by Pioneer".
    if let Some(by) = words.iter().position(|w| w.eq_ignore_ascii_case("by"))
        && by > 0
    {
        words.truncate(by);
    }
    while let Some(last) = words.last() {
        let w = last.to_lowercase();
        let before = words.len().checked_sub(2).map(|i| words[i].to_lowercase());
        if w.chars().all(|c| c.is_ascii_digit()) && before.as_deref() == Some("build") {
            // "Build 12345"; a number alone is part of the name ("Portal 2").
            words.truncate(words.len() - 2);
        } else if FIX_WORDS.contains(&w.as_str()) || is_version(&w) || matches!(w.as_str(), "repack" | "setup") {
            words.pop();
        } else {
            break;
        }
    }
    pretty_name(&words.join(" "))
}

/// The words of a file name: split on spaces, or on dashes and underscores when it has none,
/// without bracketed tags like "[FitGirl Repack]".
fn words(s: &str) -> Vec<String> {
    let mut plain = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let plain = plain.replace('_', " ");
    let plain = if plain.trim().contains(' ') { plain } else { plain.replace('-', " ") };
    plain.split_whitespace().map(str::to_owned).collect()
}

/// "v2.4", "1.0.3", "b12345"; not "2", which may be the game's.
fn is_version(w: &str) -> bool {
    let (prefixed, digits) = match w.strip_prefix(['v', 'b']) {
        Some(d) => (true, d),
        None => (false, w),
    };
    digits.starts_with(|c: char| c.is_ascii_digit())
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && (prefixed || digits.contains('.'))
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn dir_has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{tempdir, write};

    fn installer(dir: &Path, name: &str, kind: Kind) -> PathBuf {
        match kind {
            Kind::Inno => crate::testutil::inno_installer(&dir.join(name)),
            Kind::Nsis => {
                write(&dir.join(name), b"MZ\0\0\xef\xbe\xad\xdeNullsoftInst");
                dir.join(name)
            }
        }
    }

    #[test]
    fn names_the_game_from_its_installer() {
        for (path, want) in [
            ("/d/Lucid Cats v2.4 by Pioneer/setup.exe", "Lucid Cats"),
            ("/d/Lucid-Cats-Multiplayer-Fix-Online.exe", "Lucid Cats"),
            ("/d/Chained-Beasts-Multiplayer-Fix-Online.exe", "Chained Beasts"),
            ("/d/Hollow Knight [FitGirl Repack]/setup.exe", "Hollow Knight"),
            ("/d/Big_Walk_Build_12345/setup-x64.exe", "Big Walk"),
            ("/d/x/Pathogenic Setup.exe", "Pathogenic"),
            ("/d/Pong/OnlineFix.exe", "Pong"),
            ("/d/Portal 2 v1.0.3 by Someone/setup.exe", "Portal 2"),
        ] {
            assert_eq!(game_name(Path::new(path)), want, "{path}");
        }
    }

    #[test]
    fn tells_fixes_from_games() {
        assert!(looks_like_fix(Path::new("/d/Lucid-Cats-Multiplayer-Fix-Online.exe")));
        assert!(looks_like_fix(Path::new("/d/Game OnlineFix.exe")));
        assert!(!looks_like_fix(Path::new("/d/Lucid Cats v2.4 by Pioneer/setup.exe")));
        assert!(!looks_like_fix(Path::new("/d/Fixer Upper/setup.exe")));
    }

    #[test]
    fn finds_the_fix_downloaded_for_the_same_game() {
        let downloads = tempdir();
        let setup = installer(&downloads.path().join("Lucid Cats v2.4 by Pioneer"), "setup.exe", Kind::Inno);
        assert_eq!(find_fix(&setup), None);
        installer(downloads.path(), "Chained-Beasts-Multiplayer-Fix-Online.exe", Kind::Inno);
        installer(downloads.path(), "Lucid Cats Soundtrack.exe", Kind::Inno);
        assert_eq!(find_fix(&setup), None, "another game's fix, or not a fix");
        let fix = installer(downloads.path(), "Lucid-Cats-Multiplayer-Fix-Online.exe", Kind::Inno);
        assert_eq!(find_fix(&setup), Some(fix));
    }

    #[test]
    fn knows_inno_and_nsis_installers() {
        let dir = tempdir();
        assert_eq!(kind(&installer(dir.path(), "a.exe", Kind::Inno)), Ok(Kind::Inno));
        assert_eq!(kind(&installer(dir.path(), "b.exe", Kind::Nsis)), Ok(Kind::Nsis));
        write(&dir.path().join("game.exe"), "MZ just a game");
        let err = kind(&dir.path().join("game.exe")).unwrap_err();
        assert!(err.contains("game.exe"), "{err}");
    }

    #[test]
    fn runs_the_game_first_and_its_fix_after_into_games_asterofix() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let downloads = home.path().join("Downloads");
        let fix = installer(&downloads, "Lucid-Cats-Multiplayer-Fix-Online.exe", Kind::Inno);
        let setup = installer(&downloads.join("Lucid Cats v2.4 by Pioneer"), "setup.exe", Kind::Inno);
        // Picked the other way round.
        let plan = plan(&paths, &[fix.clone(), setup.clone()]).unwrap();
        assert_eq!(plan.name, "Lucid Cats");
        assert_eq!(plan.dest, home.path().join("Games/asterofix/Lucid Cats"));
        let exes: Vec<_> = plan.steps.iter().map(|s| (&s.exe, s.is_fix)).collect();
        assert_eq!(exes, [(&setup, false), (&fix, true)]);

        let proton = Proton { name: "GE".into(), dir: "/p".into(), runtime: None };
        let spec = plan.spec(&paths, 0, &proton);
        assert_eq!((spec.exe.as_path(), spec.game_dir.as_path()), (setup.as_path(), plan.dest.as_path()));
        assert_eq!(spec.prefix, default_prefix(&paths, &plan.dest), "it installs into the prefix the game plays in");
        let dir = format!("/DIR=Z:{}", plan.dest.to_string_lossy().replace('/', "\\"));
        assert_eq!(
            crate::launch::split_shell(&spec.args).unwrap(),
            ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/NOICONS", dir.as_str()]
        );
        assert!(spec.overrides.contains("winebrowser.exe=d"), "no readme opens in a browser");
        assert_ne!(spec.name, plan.spec(&paths, 1, &proton).name, "each step has its own log");

        // The setup has to leave something behind; the fix only adds to it.
        assert!(plan.check(0).is_err());
        write(&plan.dest.join("LucidCats.exe"), "x");
        assert_eq!(plan.check(0), Ok(()));
        assert_eq!(plan.check(1), Ok(()));
    }

    #[test]
    fn nsis_takes_the_folder_last_and_unquoted() {
        let args = silent_args(Kind::Nsis, Path::new("/home/me/Games/asterofix/A B"));
        assert_eq!(args, ["/S", r"/D=Z:\home\me\Games\asterofix\A B"]);
    }

    #[test]
    fn never_installs_over_a_game_and_fixes_need_one() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let fix = installer(home.path(), "Pong-Fix.exe", Kind::Inno);
        let err = plan(&paths, std::slice::from_ref(&fix)).unwrap_err();
        assert!(err.contains("install Pong first"), "{err}");

        let dest = home.path().join("Games/asterofix/Pong");
        write(&dest.join("Pong.exe"), "x");
        let only_fix = plan(&paths, std::slice::from_ref(&fix)).unwrap();
        assert_eq!(only_fix.dest, dest, "a fix alone goes into its game's folder");

        let setup = installer(&home.path().join("Pong v1.0"), "setup.exe", Kind::Inno);
        let err = plan(&paths, std::slice::from_ref(&setup)).unwrap_err();
        assert!(err.contains("already exists"), "{err}");

        let other = installer(&home.path().join("Other"), "setup.exe", Kind::Inno);
        assert!(plan(&paths, &[setup, other]).unwrap_err().contains("one game"));
        assert!(plan(&paths, &[]).is_err());
    }
}
