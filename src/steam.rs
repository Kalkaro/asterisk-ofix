//! Finding Steam, its Proton builds, and the running client.

use crate::vdf;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Steam {
    pub root: PathBuf,
    /// Whose namespaces games are launched in: 0 for the running Steam client. Tests point
    /// it at a fake Steam.
    pub sandbox_pid: i32,
}

/// A compatibility tool games can run with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proton {
    /// Display name, also the key saved in settings.
    pub name: String,
    /// Contains the `proton` script.
    pub dir: PathBuf,
    /// The Steam Linux Runtime entry point the tool expects to run inside, if any.
    pub runtime: Option<PathBuf>,
}

pub fn detect(home: &Path) -> Option<Steam> {
    [".local/share/Steam", ".steam/steam", ".steam/root"].iter().find_map(|p| {
        let root = std::fs::canonicalize(home.join(p)).ok()?;
        root.join("steamapps").is_dir().then_some(Steam { root, sandbox_pid: 0 })
    })
}

impl Steam {
    pub fn target_pid(&self) -> i32 {
        if self.sandbox_pid != 0 { self.sandbox_pid } else { steam_client_pid() }
    }

    /// Custom tools (newest first), then Valve's builds.
    pub fn protons(&self) -> Vec<Proton> {
        let mut seen = std::collections::HashSet::new();
        let mut add = |list: &mut Vec<Proton>, name: String, dir: PathBuf| {
            if !seen.contains(&name) && dir.join("proton").exists() {
                seen.insert(name.clone());
                list.push(Proton { runtime: self.runtime_for(&dir), name, dir });
            }
        };

        let mut custom = Vec::new();
        for base in [self.root.join("compatibilitytools.d"), PathBuf::from("/usr/share/steam/compatibilitytools.d")] {
            for dir in crate::scan::read_dir_sorted(&base) {
                let manifest = vdf::read(&dir.join("compatibilitytool.vdf"));
                for (key, tool) in &manifest.path(&["compatibilitytools", "compat_tools"]).0 {
                    let vdf::Value::Obj(tool) = tool else { continue };
                    let name = match tool.str("display_name") {
                        "" => key.clone(),
                        n => n.to_owned(),
                    };
                    let tool_dir = match tool.str("install_path") {
                        "" | "." => dir.clone(),
                        install => dir.join(install),
                    };
                    add(&mut custom, name, tool_dir);
                }
            }
        }
        custom.sort_by(|a, b| natural_cmp(&b.name, &a.name));

        let mut valve = Vec::new();
        let common = self.root.join("steamapps/common");
        for dir in crate::scan::read_dir_sorted(&common) {
            let name = crate::scan::file_name(&dir);
            if name.starts_with("Proton") {
                add(&mut valve, name, dir);
            }
        }
        valve.sort_by(|a, b| natural_cmp(&b.name, &a.name));
        custom.extend(valve);
        custom
    }

    /// The Steam Linux Runtime a tool declares in its toolmanifest.vdf (e.g. sniper), as that
    /// runtime's entry point.
    fn runtime_for(&self, tool_dir: &Path) -> Option<PathBuf> {
        let manifest = vdf::read(&tool_dir.join("toolmanifest.vdf"));
        let app_id = manifest.obj("manifest").str("require_tool_appid");
        if app_id.is_empty() {
            return None;
        }
        let app = vdf::read(&self.root.join(format!("steamapps/appmanifest_{app_id}.acf")));
        let install_dir = app.obj("AppState").str("installdir");
        if install_dir.is_empty() {
            return None;
        }
        let entry = self.root.join("steamapps/common").join(install_dir).join("_v2-entry-point");
        entry.exists().then_some(entry)
    }

    /// The tool Steam uses for "all other titles", if we have it.
    pub fn default_proton(&self, protons: &[Proton]) -> String {
        let config = vdf::read(&self.root.join("config/config.vdf"));
        let mapping = config.path(&["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping", "0"]);
        let name = match mapping.str("name") {
            "proton_experimental" => "Proton - Experimental",
            "proton_hotfix" => "Proton Hotfix",
            n => n,
        };
        protons
            .iter()
            .find(|p| p.name == name || crate::scan::file_name(&p.dir) == name)
            .or(protons.first())
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }
}

/// Whether a Steam client process exists.
pub fn steam_running() -> bool {
    steam_pids().next().is_some()
}

/// The PID of the running Steam client binary, or 0.
pub fn steam_client_pid() -> i32 {
    steam_pids()
        .find(|pid| {
            std::fs::read_link(format!("/proc/{pid}/exe"))
                .is_ok_and(|exe| exe.to_string_lossy().contains("ubuntu12_32/steam"))
        })
        .unwrap_or(0)
}

fn steam_pids() -> impl Iterator<Item = i32> {
    std::fs::read_dir("/proc").into_iter().flatten().filter_map(|e| {
        let pid: i32 = e.ok()?.file_name().to_str()?.parse().ok()?;
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        (comm.trim() == "steam").then_some(pid)
    })
}

/// An nsenter prefix that runs a command inside the namespaces of `pid` (the Steam client),
/// or None when they match ours. On NixOS Steam lives in a bubblewrap FHS sandbox with a
/// private /tmp and its own PID namespace; the overlay exchanges frames through /tmp and
/// tracks games by PID, so a game started outside that sandbox grabs input but never
/// renders the overlay.
pub fn namespace_wrapper(pid: i32) -> Option<Vec<String>> {
    if pid <= 0 {
        return None;
    }
    let nsenter = look_path("nsenter")?;
    let flags = namespace_flags(|ns| {
        match (std::fs::read_link(format!("/proc/self/ns/{ns}")), std::fs::read_link(format!("/proc/{pid}/ns/{ns}"))) {
            (Ok(me), Ok(them)) => me != them,
            _ => false,
        }
    });
    if flags.is_empty() {
        return None;
    }
    let mut argv = vec![nsenter.to_string_lossy().into_owned(), "--target".into(), pid.to_string()];
    argv.extend(flags.iter().map(|f| f.to_string()));
    argv.push("--".into());
    Some(argv)
}

/// The nsenter flags for the namespaces that differ.
pub fn namespace_flags(differs: impl Fn(&str) -> bool) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if differs("user") {
        flags.extend(["--user", "--preserve-credentials"]);
    }
    if differs("mnt") {
        flags.extend(["--mount", "--root"]);
    }
    if differs("ipc") {
        flags.push("--ipc");
    }
    if differs("pid") {
        flags.push("--pid");
    }
    flags
}

/// Starts the Steam client minimized to the tray.
pub fn start_steam() -> std::io::Result<()> {
    let mut child = std::process::Command::new("steam").arg("-silent").spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Finds an executable on PATH.
pub fn look_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// Compares strings with embedded numbers numerically ("GE-Proton9-27" < "GE-Proton10-4").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(&cb) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(xn), Ok(yn)) => xn.cmp(&yn),
            _ => x.to_lowercase().cmp(&y.to_lowercase()),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    ca.len().cmp(&cb.len())
}

fn chunks(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let digit = c.is_ascii_digit();
        if prev.is_some_and(|p| p != digit) {
            out.push(&s[start..i]);
            start = i;
        }
        prev = Some(digit);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{tempdir, write};

    #[test]
    fn natural_ordering() {
        assert_eq!(natural_cmp("GE-Proton9-27", "GE-Proton10-4"), Ordering::Less);
        assert_eq!(natural_cmp("GE-Proton10-34", "GE-Proton10-4"), Ordering::Greater);
        assert_eq!(natural_cmp("Proton 9.0", "proton 9.0"), Ordering::Equal);
    }

    #[test]
    fn namespace_flags_for_each_namespace() {
        assert_eq!(
            namespace_flags(|_| true),
            ["--user", "--preserve-credentials", "--mount", "--root", "--ipc", "--pid"]
        );
        assert!(namespace_flags(|_| false).is_empty());
        assert_eq!(namespace_wrapper(std::process::id() as i32), None, "our own process needs no wrapper");
        assert_eq!(namespace_wrapper(0), None, "no Steam needs no wrapper");
    }

    #[test]
    fn lists_protons_with_their_runtime_and_the_default() {
        let home = tempdir();
        let root = home.path().join(".local/share/Steam");
        let tools = root.join("compatibilitytools.d");
        for v in ["GE-Proton9-27", "GE-Proton10-4"] {
            write(&tools.join(v).join("proton"), "");
            write(
                &tools.join(v).join("compatibilitytool.vdf"),
                format!(
                    r#""compatibilitytools" {{ "compat_tools" {{ "{v}" {{ "install_path" "." "display_name" "{v}" }} }} }}"#
                ),
            );
            write(&tools.join(v).join("toolmanifest.vdf"), r#""manifest" { "require_tool_appid" "1628350" }"#);
        }
        write(
            &root.join("steamapps/appmanifest_1628350.acf"),
            r#""AppState" { "installdir" "SteamLinuxRuntime_sniper" }"#,
        );
        write(&root.join("steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point"), "");
        write(&root.join("steamapps/common/Proton - Experimental/proton"), "");
        write(
            &root.join("config/config.vdf"),
            r#""InstallConfigStore" { "Software" { "Valve" { "Steam" { "CompatToolMapping" { "0" { "name" "proton_experimental" } } } } } }"#,
        );

        let steam = detect(home.path()).unwrap();
        let protons = steam.protons();
        let names: Vec<_> = protons.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["GE-Proton10-4", "GE-Proton9-27", "Proton - Experimental"]);
        assert_eq!(
            protons[0].runtime,
            Some(steam.root.join("steamapps/common/SteamLinuxRuntime_sniper/_v2-entry-point"))
        );
        assert_eq!(protons[2].runtime, None);
        assert_eq!(steam.default_proton(&protons), "Proton - Experimental");
        assert!(detect(tempdir().path()).is_none());
    }
}
