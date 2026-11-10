//! The user's libraries, hand-picked games and per-game overrides, in config.json.

use crate::paths::Paths;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
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
        let path = paths.config_dir().join("config.json");
        let error = match std::fs::read(&path) {
            Ok(data) => match serde_json::from_slice(&data) {
                Ok(c) => return (c, None),
                Err(e) => e.to_string(),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Config::fresh(paths), None),
            Err(e) => e.to_string(),
        };
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let aside = path.with_file_name(format!("config.json.broken-{secs}"));
        let warning = match std::fs::rename(&path, &aside) {
            Ok(()) => format!("Unreadable config.json kept as {} ({error})", crate::scan::file_name(&aside)),
            Err(e) => format!("Unreadable config.json ({error}), and moving it aside failed: {e}"),
        };
        (Config::fresh(paths), Some(warning))
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

    pub fn save(&self, paths: &Paths) -> std::io::Result<()> {
        let dir = paths.config_dir();
        std::fs::create_dir_all(&dir)?;
        let data = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = dir.join("config.json.part");
        std::fs::write(&tmp, data)?;
        std::fs::rename(tmp, dir.join("config.json"))
    }

    pub fn settings(&self, root: &std::path::Path) -> GameSettings {
        self.games.get(root).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;

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
        Config { libraries: vec!["/h".into()], ..c }.save(&paths).unwrap();
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
        let (c, warning) = Config::open(&paths);
        assert!(c.libraries.is_empty());
        let warning = warning.expect("a warning about the broken config");
        assert!(warning.contains("config.json"), "{warning}");

        c.save(&paths).unwrap();
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
        Config::default().save(&paths).unwrap();
        let saved = std::fs::read_to_string(paths.config_dir().join("config.json")).unwrap();
        assert!(!saved.contains("language"), "{saved}");
        Config { language: "pt-BR".into(), ..Config::default() }.save(&paths).unwrap();
        assert_eq!(Config::load(&paths).language, "pt-BR");
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
