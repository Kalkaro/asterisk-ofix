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
}

/// Empty lists may be written as `null`.
fn null_as_default<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

impl Config {
    /// Reads the saved config, or starts with `~/Games`.
    pub fn load(paths: &Paths) -> Config {
        if let Ok(data) = std::fs::read(paths.config_dir().join("config.json")) {
            return serde_json::from_slice(&data).unwrap_or_default();
        }
        let games = paths.home.join("Games");
        Config { libraries: if games.is_dir() { vec![games] } else { Vec::new() }, ..Config::default() }
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

        // Saving writes asterisk-ofix's own file, which wins from then on.
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
    }

    #[test]
    fn empty_settings_are_left_out() {
        let json = serde_json::to_string(&GameSettings { env: "A=1".into(), ..Default::default() }).unwrap();
        assert_eq!(json, r#"{"env":"A=1"}"#);
    }
}
