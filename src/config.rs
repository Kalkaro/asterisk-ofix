//! The user's libraries, hand-picked games and per-game overrides, in config.json.

use crate::paths::Paths;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// A game's overrides; empty fields mean "use what was detected".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exe: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub proton: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub fake_app_id: u32,
    #[serde(rename = "dll_overrides", skip_serializing_if = "String::is_empty")]
    pub overrides: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub env: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub args: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    /// The game page stops offering saves found in an old prefix (`prefixes::old_saves`): the
    /// user said they aren't this game's.
    #[serde(skip_serializing_if = "is_false")]
    pub hide_old_saves: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl GameSettings {
    pub fn is_empty(&self) -> bool {
        *self == GameSettings::default()
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(deserialize_with = "null_as_default")]
    pub libraries: Vec<PathBuf>,
    #[serde(deserialize_with = "null_as_default")]
    pub manual_exes: Vec<PathBuf>,
    /// Keyed by the game's root folder.
    #[serde(deserialize_with = "null_as_default")]
    pub games: BTreeMap<PathBuf, GameSettings>,
    /// The UI's language code, picked in the app; empty follows the system's locale.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// `false` stops the AppImage asking GitHub whether a newer release is out (`update`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_for_updates: Option<bool>,
}

enum OnDisk {
    Read(Config),
    Missing,
    /// What's wrong with it.
    Broken(String),
}

fn read(paths: &Paths) -> OnDisk {
    match std::fs::read(paths.config_dir().join("config.json")) {
        Ok(data) => match serde_json::from_slice(&data) {
            Ok(c) => OnDisk::Read(c),
            Err(e) => OnDisk::Broken(e.to_string()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => OnDisk::Missing,
        Err(e) => OnDisk::Broken(e.to_string()),
    }
}

/// Moves an unreadable config.json aside, so nothing writes over the user's settings, and says
/// where it went.
fn keep_aside(paths: &Paths, error: &str) -> String {
    let path = paths.config_dir().join("config.json");
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let aside = path.with_file_name(format!("config.json.broken-{secs}"));
    match std::fs::rename(&path, &aside) {
        Ok(()) => format!("Unreadable config.json kept as {} ({error})", crate::scan::file_name(&aside)),
        Err(e) => format!("Unreadable config.json ({error}), and moving it aside failed: {e}"),
    }
}

/// Runs `f` holding config.json's lock, so two processes never read and write it at once. The
/// kernel drops an flock when its holder dies, so a crash can't leave it locked.
fn locked<T>(paths: &Paths, f: impl FnOnce() -> T) -> std::io::Result<T> {
    use std::os::fd::AsRawFd;
    let dir = paths.config_dir();
    std::fs::create_dir_all(&dir)?;
    let lock =
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("config.json.lock"))?;
    // SAFETY: flock on a descriptor this function owns; closing it unlocks.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(f())
}

/// `ours` and `theirs` both started from `base`; the result has the changes of both. It's
/// done on their JSON, so every field takes part, ones added later included: objects (the
/// games, a game's settings) merge key by key, and lists (libraries, added .exes) as sets,
/// keeping what either added and dropping what either removed. Where both changed the same
/// value, ours wins: it's the edit being saved.
fn merge_configs(base: &Config, ours: &Config, theirs: &Config) -> Config {
    let json = |c: &Config| serde_json::to_value(c).unwrap_or_default();
    let merged = merge(Some(&json(base)), Some(&json(ours)), Some(&json(theirs)));
    merged.and_then(|m| serde_json::from_value(m).ok()).unwrap_or_else(|| ours.clone())
}

/// One value of a three-way merge; None is a key that isn't there.
fn merge(base: Option<&Value>, ours: Option<&Value>, theirs: Option<&Value>) -> Option<Value> {
    if ours == base {
        return theirs.cloned();
    }
    if theirs == base || theirs == ours {
        return ours.cloned();
    }
    match (ours, theirs) {
        (Some(Value::Object(o)), Some(Value::Object(t))) => {
            let b = base.and_then(Value::as_object);
            let keys: BTreeSet<&String> =
                o.keys().chain(t.keys()).chain(b.into_iter().flat_map(|b| b.keys())).collect();
            let merged = keys.into_iter().filter_map(|k| {
                let v = merge(b.and_then(|b| b.get(k)), o.get(k), t.get(k))?;
                Some((k.clone(), v))
            });
            Some(Value::Object(merged.collect()))
        }
        (Some(Value::Array(o)), Some(Value::Array(t))) => {
            let b = base.and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            // Theirs in its order, less what we removed, then what we added.
            let mut out: Vec<Value> = t.iter().filter(|v| o.contains(v) || !b.contains(v)).cloned().collect();
            for v in o.iter().filter(|v| !b.contains(v)) {
                if !out.contains(v) {
                    out.push(v.clone());
                }
            }
            Some(Value::Array(out))
        }
        _ => ours.cloned(),
    }
}

/// Empty lists may be written as `null`.
fn null_as_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

impl Config {
    /// Reads the saved config, or starts with `~/Games`.
    #[cfg(test)]
    pub fn load(paths: &Paths) -> Config {
        Config::open(paths).0
    }

    /// `load`, and what went wrong if the saved config couldn't be read. That file is moved
    /// aside, so saving the fresh config can't overwrite the user's settings.
    pub fn open(paths: &Paths) -> (Config, Option<String>) {
        let read = locked(paths, || match read(paths) {
            OnDisk::Read(c) => (c, None),
            OnDisk::Missing => (Config::fresh(paths), None),
            OnDisk::Broken(error) => (Config::fresh(paths), Some(keep_aside(paths, &error))),
        });
        read.unwrap_or_else(|e| (Config::fresh(paths), Some(format!("Could not read config.json: {e}"))))
    }

    fn fresh(paths: &Paths) -> Config {
        let libraries = [paths.home.join("Games"), paths.home.join(crate::install::INSTALL_DIR)];
        Config { libraries: libraries.into_iter().filter(|d| d.is_dir()).collect(), ..Config::default() }
    }

    /// Adds a library folder; false when it was one already.
    pub fn add_library(&mut self, dir: PathBuf) -> bool {
        let new = !self.libraries.contains(&dir);
        if new {
            self.libraries.push(dir);
        }
        new
    }

    /// Writes this config to config.json, keeping what another process saved there since
    /// `base` was read: the window and `--install` can run at once, and each holds its own
    /// copy. Under a lock, it reads the file again and merges three ways (`merge_configs`): what this
    /// copy changed since `base` goes over what's on disk, and the rest of the disk's version
    /// stays. Afterwards this config and `base` are both what was written. A file that no
    /// longer parses is kept aside, as `open` does, and the warning returned.
    pub fn save(&mut self, base: &mut Config, paths: &Paths) -> std::io::Result<Option<String>> {
        let dir = paths.config_dir();
        locked(paths, || {
            let (theirs, warning) = match read(paths) {
                OnDisk::Read(c) => (c, None),
                OnDisk::Missing => (base.clone(), None),
                OnDisk::Broken(error) => (base.clone(), Some(keep_aside(paths, &error))),
            };
            let merged = merge_configs(base, self, &theirs);
            let data = serde_json::to_vec_pretty(&merged).map_err(std::io::Error::other)?;
            let tmp = dir.join("config.json.part");
            std::fs::write(&tmp, data)?;
            std::fs::rename(tmp, dir.join("config.json"))?;
            *base = merged.clone();
            *self = merged;
            Ok(warning)
        })?
    }

    /// Takes in what another process saved since `base` was read, keeping this copy's own
    /// changes; true when anything changed. A file that doesn't parse is left alone: the user
    /// may be in the middle of editing it.
    pub fn reload(&mut self, base: &mut Config, paths: &Paths) -> bool {
        let Ok(OnDisk::Read(theirs)) = locked(paths, || read(paths)) else { return false };
        if theirs == *base {
            return false;
        }
        let merged = merge_configs(base, self, &theirs);
        let changed = merged != *self;
        *self = merged;
        *base = theirs;
        changed
    }

    /// Adds a library to config.json, as `--install` does once a game is in. Returns a warning
    /// if the file had to be kept aside.
    pub fn add_saved_library(paths: &Paths, dir: PathBuf) -> std::io::Result<Option<String>> {
        let (mut cfg, warning) = Config::open(paths);
        let mut base = cfg.clone();
        // With no config.json yet, the fresh config has the install folder already; it's
        // written all the same, or a window that started before the folder existed never
        // learns of it.
        let written = paths.config_dir().join("config.json").exists();
        if !cfg.add_library(dir) && written {
            return Ok(warning);
        }
        Ok(cfg.save(&mut base, paths)?.or(warning))
    }

    pub fn settings(&self, root: &std::path::Path) -> GameSettings {
        self.games.get(root).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;
    use std::path::Path;

    /// Saves `c` as a copy that started from `base`.
    fn save_over(mut c: Config, base: &Config, paths: &Paths) -> Config {
        c.save(&mut base.clone(), paths).unwrap();
        c
    }

    #[test]
    fn reads_a_config_with_nulls() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        std::fs::create_dir_all(paths.config_dir()).unwrap();
        std::fs::write(
            paths.config_dir().join("config.json"),
            r#"{"libraries":["/g"],"manual_exes":null,"games":{"/g/A":{"args":"-w","fake_app_id":480}}}"#,
        )
        .unwrap();
        let c = Config::load(&paths);
        assert_eq!(c.libraries, vec![PathBuf::from("/g")]);
        assert!(c.manual_exes.is_empty());
        assert_eq!(
            c.settings("/g/A".as_ref()),
            GameSettings { args: "-w".into(), fake_app_id: 480, ..Default::default() }
        );

        // Saving writes asterofix's own file, which wins from then on.
        save_over(Config { libraries: vec!["/h".into()], ..c.clone() }, &c, &paths);
        assert_eq!(Config::load(&paths).libraries, vec![PathBuf::from("/h")]);
    }

    #[test]
    fn a_fresh_config_uses_the_games_folder() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        assert!(Config::load(&paths).libraries.is_empty());
        std::fs::create_dir(home.path().join("Games")).unwrap();
        assert_eq!(Config::load(&paths).libraries, vec![home.path().join("Games")]);
        // Where games are installed to.
        std::fs::create_dir(home.path().join("Games/asterofix")).unwrap();
        assert_eq!(Config::load(&paths).libraries, [home.path().join("Games"), home.path().join("Games/asterofix")]);
    }

    /// A config that doesn't parse used to load as empty, and the next save wrote that over
    /// every library and game setting.
    #[test]
    fn a_broken_config_is_kept_aside() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let broken = r#"{"libraries":["/g"],"games":{"/g/A":{"fake_app_id":"480"}}}"#;
        crate::testutil::write(&paths.config_dir().join("config.json"), broken);
        let (mut c, warning) = Config::open(&paths);
        assert!(c.libraries.is_empty());
        let warning = warning.expect("a warning about the broken config");
        assert!(warning.contains("config.json"), "{warning}");

        c.save(&mut c.clone(), &paths).unwrap();
        let kept: Vec<_> = std::fs::read_dir(paths.config_dir())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| crate::scan::file_name(p).starts_with("config.json.broken"))
            .collect();
        assert_eq!(kept.len(), 1, "the broken file is kept beside the new one");
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), broken);
        assert!(warning.contains(&crate::scan::file_name(&kept[0])), "{warning}");
        assert_eq!(Config::open(&paths).1, None, "the new config reads fine");
    }

    #[test]
    fn the_language_is_saved_only_once_picked() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        save_over(Config::default(), &Config::default(), &paths);
        let saved = std::fs::read_to_string(paths.config_dir().join("config.json")).unwrap();
        assert!(!saved.contains("language"), "{saved}");
        save_over(Config { language: "pt-BR".into(), ..Config::default() }, &Config::default(), &paths);
        assert_eq!(Config::load(&paths).language, "pt-BR");
    }

    /// The window and `--install` each hold a copy of the config. The window used to save its
    /// whole copy, dropping the library an install had added since it started.
    #[test]
    fn a_save_keeps_what_another_process_saved() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        save_over(Config { libraries: vec!["/g".into()], ..Config::default() }, &Config::default(), &paths);
        let (mut window, _) = Config::open(&paths);
        let mut base = window.clone();

        // `--install` adds its library meanwhile.
        assert_eq!(Config::add_saved_library(&paths, "/installed".into()).unwrap(), None);

        // The window's next edit keeps it, and the window takes it in.
        window.games.insert("/g/A".into(), GameSettings { args: "-w".into(), ..Default::default() });
        assert_eq!(window.save(&mut base, &paths).unwrap(), None);
        let saved = Config::load(&paths);
        assert_eq!(saved.libraries, [PathBuf::from("/g"), PathBuf::from("/installed")]);
        assert_eq!(saved.settings("/g/A".as_ref()).args, "-w");
        assert_eq!(window, saved);
        assert_eq!(base, saved);

        // And the other way round: an install that read the config before the window's edit.
        let (mut install, _) = Config::open(&paths);
        let mut install_base = install.clone();
        window.language = "de".into();
        window.save(&mut base, &paths).unwrap();
        install.add_library("/more".into());
        install.save(&mut install_base, &paths).unwrap();
        let saved = Config::load(&paths);
        assert_eq!(saved.language, "de");
        assert_eq!(saved.libraries.len(), 3);
        assert_eq!(saved.settings("/g/A".as_ref()).args, "-w");
    }

    #[test]
    fn merging_keeps_both_sides_changes() {
        let lib = |dirs: &[&str]| dirs.iter().map(PathBuf::from).collect::<Vec<_>>();
        let game =
            |args: &str, proton: &str| GameSettings { args: args.into(), proton: proton.into(), ..Default::default() };
        let base = Config {
            libraries: lib(&["/a", "/b"]),
            games: [("/a/X".into(), game("-x", "")), ("/a/Y".into(), game("-y", ""))].into(),
            language: "fr".into(),
            ..Config::default()
        };
        // We removed /a and a game's settings, and changed a game's Proton and the language.
        let ours = Config {
            libraries: lib(&["/b"]),
            games: [("/a/X".into(), game("-x", "GE"))].into(),
            language: "de".into(),
            ..Config::default()
        };
        // They added /c, a game and an .exe, changed the same game's arguments, and the language.
        let theirs = Config {
            libraries: lib(&["/a", "/b", "/c"]),
            manual_exes: lib(&["/z/z.exe"]),
            games: [("/a/X".into(), game("-xx", "")), ("/a/Y".into(), game("-y", "")), ("/c/Z".into(), game("-z", ""))]
                .into(),
            language: "ja".into(),
            check_for_updates: Some(false),
        };
        let merged = merge_configs(&base, &ours, &theirs);
        assert_eq!(merged.check_for_updates, Some(false));
        assert_eq!(merged.libraries, lib(&["/b", "/c"]));
        assert_eq!(merged.manual_exes, lib(&["/z/z.exe"]));
        assert_eq!(merged.settings("/a/X".as_ref()), game("-xx", "GE"), "each side's own field");
        assert!(!merged.games.contains_key(Path::new("/a/Y")), "we removed it");
        assert_eq!(merged.settings("/c/Z".as_ref()), game("-z", ""));
        assert_eq!(merged.language, "de", "where both changed a value, the edit being saved wins");
        // Nothing changed on one side: the other's copy, exactly.
        assert_eq!(merge_configs(&base, &base, &theirs), theirs);
        assert_eq!(merge_configs(&base, &ours, &base), ours);
    }

    /// The window takes in what `--install` saved without waiting for an edit of its own, and
    /// keeps a change it couldn't save.
    #[test]
    fn reloading_takes_in_other_saves() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        save_over(Config::default(), &Config::default(), &paths);
        let (mut window, _) = Config::open(&paths);
        let mut base = window.clone();
        assert!(!window.reload(&mut base, &paths), "nothing new");
        window.language = "uk".into();
        Config::add_saved_library(&paths, "/installed".into()).unwrap();
        assert!(window.reload(&mut base, &paths));
        assert_eq!(window.libraries, [PathBuf::from("/installed")]);
        assert_eq!(window.language, "uk", "the unsaved change stays");
        assert_eq!(base, Config::load(&paths));
        // A file the user is halfway through editing is left alone.
        crate::testutil::write(&paths.config_dir().join("config.json"), "{");
        assert!(!window.reload(&mut base, &paths));
        assert!(paths.config_dir().join("config.json").exists());
    }

    /// A file broken after the window read it is kept aside, as at startup.
    #[test]
    fn a_save_keeps_a_newly_broken_config_aside() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let (mut window, _) = Config::open(&paths);
        let mut base = window.clone();
        crate::testutil::write(&paths.config_dir().join("config.json"), "{ half");
        window.language = "pl".into();
        let warning = window.save(&mut base, &paths).unwrap().expect("a warning");
        assert!(warning.contains("kept as config.json.broken-"), "{warning}");
        assert_eq!(Config::load(&paths).language, "pl");
    }

    #[test]
    fn empty_settings_are_left_out() {
        let json = serde_json::to_string(&GameSettings { env: "A=1".into(), ..Default::default() }).unwrap();
        assert_eq!(json, r#"{"env":"A=1"}"#);
    }

    /// Configs from before the notice about old saves load as offering them.
    #[test]
    fn dismissing_old_saves_is_remembered() {
        let old: GameSettings = serde_json::from_str(r#"{"prefix":""}"#).unwrap();
        assert!(!old.hide_old_saves);
        let s = GameSettings { hide_old_saves: true, ..Default::default() };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"hide_old_saves":true}"#);
        assert_eq!(serde_json::from_str::<GameSettings>(&json).unwrap(), s);
    }
}
