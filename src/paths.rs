//! Where asterofix keeps its config, data and cache. Everything that touches the user's files
//! takes a `Paths`, so tests point it at a temp dir instead of changing `HOME` (tests run in
//! parallel threads that share one environment).

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

pub const APP: &str = "asterofix";

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    config_base: PathBuf,
    data_base: PathBuf,
    cache_base: PathBuf,
}

impl Paths {
    /// The real locations, from `HOME` and the XDG variables.
    pub fn from_env() -> Paths {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
        let xdg = |var: &str, rel: &str| match std::env::var_os(var).map(PathBuf::from) {
            Some(p) if p.is_absolute() => p,
            _ => home.join(rel),
        };
        Paths {
            config_base: xdg("XDG_CONFIG_HOME", ".config"),
            data_base: xdg("XDG_DATA_HOME", ".local/share"),
            cache_base: xdg("XDG_CACHE_HOME", ".cache"),
            home,
        }
    }

    /// The default XDG layout under `home`.
    #[cfg(test)]
    pub fn under(home: &Path) -> Paths {
        Paths {
            home: home.to_path_buf(),
            config_base: home.join(".config"),
            data_base: home.join(".local/share"),
            cache_base: home.join(".cache"),
        }
    }

    pub fn config_dir(&self) -> PathBuf {
        self.config_base.join(APP)
    }
    pub fn data_dir(&self) -> PathBuf {
        self.data_base.join(APP)
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.cache_base.join(APP)
    }
}
