//! Building and running the command that starts a game through Proton.

use crate::paths::Paths;
use crate::scan::Game;
use crate::steam::{Proton, Steam, look_path, namespace_wrapper};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Everything needed to start one game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchSpec {
    pub name: String,
    pub exe: PathBuf,
    pub game_dir: PathBuf,
    pub proton: Proton,
    pub prefix: PathBuf,
    pub fake_app_id: u32,
    pub overrides: String,
    /// Extra KEY=VALUE pairs, shell-style quoting.
    pub env: String,
    /// Extra game arguments.
    pub args: String,
}

pub type Env = Vec<(String, String)>;

/// Called from a background thread when the game's Proton process ends.
pub type OnExit = Box<dyn FnOnce(Result<(), String>) + Send>;

/// Set to the game's folder in everything a launch starts. Proton, Wine and the game inherit
/// it, inside Steam's sandbox too, where they aren't our children: Stop finds them by it.
pub const GAME_MARKER: &str = "ASTERISK_OFIX_GAME";

/// A launch's argv, knowing which of its arguments are KEY=VALUE pairs passed on from the
/// session (or Steam's), so printing it never shows their values.
#[derive(Clone, Debug)]
pub struct LaunchCommand {
    pub argv: Vec<String>,
    inherited: Vec<bool>,
}

/// Inherited variables printed with their values: joining Steam's sandbox they are Steam's,
/// which a terminal the command is pasted into may not share, and they hold nothing private.
const PRINTED_AS_IS: &[&str] = &["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"];

impl LaunchCommand {
    /// As a copy-pasteable shell line. Variables passed on from the session appear as
    /// `KEY="$KEY"`: the line still runs in the user's shell, but values that `is_secret_env`
    /// can't recognise (a password inside a URL, a token by another name) don't end up in logs
    /// and bug reports. One whose name the shell can't expand is left out.
    pub fn shell_line(&self) -> String {
        self.printed(shell_quote)
    }

    /// The line, with `own` printing every argument that isn't a hidden inherited variable.
    fn printed(&self, own: impl Fn(&str) -> String) -> String {
        let mut out = Vec::new();
        for (a, &inherited) in self.argv.iter().zip(&self.inherited) {
            match a.split_once('=') {
                Some((k, _)) if inherited && !PRINTED_AS_IS.contains(&k) => {
                    if is_shell_name(k) {
                        out.push(format!("{k}=\"${k}\""));
                    }
                }
                _ => out.push(own(a)),
            }
        }
        out.join(" ")
    }
}

fn is_shell_name(k: &str) -> bool {
    k.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Steam {
    /// Just the argv of `launch_command`.
    #[cfg(test)]
    pub fn command(&self, spec: &LaunchSpec) -> Result<Vec<String>, String> {
        Ok(self.launch_command(spec)?.argv)
    }

    /// Just the argv of `launch_command_from`.
    #[cfg(test)]
    pub fn command_from(&self, spec: &LaunchSpec, host: Env) -> Result<Vec<String>, String> {
        Ok(self.launch_command_from(spec, host)?.argv)
    }

    /// The command that starts the game, mirroring what Steam does for a Proton game: run the
    /// `proton` script inside the Steam Linux Runtime, with the overlay preloaded and the
    /// Steam game IDs set to OnlineFix's fake app ID.
    pub fn launch_command(&self, spec: &LaunchSpec) -> Result<LaunchCommand, String> {
        self.launch_command_from(spec, std::env::vars_os().map(|(k, v)| lossy(k, v)).collect())
    }

    /// `launch_command` with `host` standing in for this process's environment.
    pub fn launch_command_from(&self, spec: &LaunchSpec, host: Env) -> Result<LaunchCommand, String> {
        if spec.proton.dir.as_os_str().is_empty() {
            return Err("no Proton build selected".into());
        }
        let extra_args = split_shell(&spec.args).map_err(|e| format!("arguments: {e}"))?;
        let extra_env = split_shell(&spec.env).map_err(|e| format!("environment: {e}"))?;
        if let Some(bad) = extra_env.iter().find(|kv| !kv.contains('=')) {
            return Err(format!("environment: {bad:?} is not KEY=VALUE"));
        }

        let proton = path_str(&spec.proton.dir.join("proton"));
        let exe = path_str(&spec.exe);
        let mut game = match &spec.proton.runtime {
            Some(rt) => vec![
                path_str(rt),
                "--verb=waitforexitandrun".into(),
                "--".into(),
                proton,
                "waitforexitandrun".into(),
                exe,
            ],
            None => vec![proton, "waitforexitandrun".into(), exe],
        };
        game.extend(extra_args);

        // The game gets the environment Steam would give it: Steam's own (display, X auth,
        // runtime dir valid inside its sandbox) when we can join Steam, otherwise ours.
        let pid = self.target_pid();
        let wrap = namespace_wrapper(pid);
        let base = match &wrap {
            Some(_) => Some(proc_environ(pid)).filter(|e| !e.is_empty()).unwrap_or(host),
            None => host,
        };

        let app_id = spec.fake_app_id.to_string();
        let mut tool_paths = path_str(&spec.proton.dir);
        if let Some(rt) = &spec.proton.runtime {
            tool_paths = format!("{tool_paths}:{}", path_str(rt.parent().unwrap_or(Path::new("/"))));
        }
        let root = &self.root;
        let host_path = base.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.as_str()).unwrap_or("");
        let set: BTreeMap<&str, String> = [
            ("STEAM_COMPAT_CLIENT_INSTALL_PATH", path_str(root)),
            ("STEAM_COMPAT_DATA_PATH", path_str(&spec.prefix)),
            ("STEAM_COMPAT_INSTALL_PATH", path_str(&spec.game_dir)),
            ("STEAM_COMPAT_LIBRARY_PATHS", path_str(&root.join("steamapps"))),
            ("STEAM_COMPAT_TOOL_PATHS", tool_paths),
            ("STEAM_COMPAT_APP_ID", app_id.clone()),
            ("SteamAppId", app_id.clone()),
            ("SteamGameId", app_id.clone()),
            ("SteamOverlayGameId", app_id),
            ("SteamEnv", "1".into()),
            ("ENABLE_VK_LAYER_VALVE_steam_overlay_1", "1".into()),
            ("WINEDLLOVERRIDES", spec.overrides.clone()),
            (GAME_MARKER, path_str(&spec.game_dir)),
            // FHS paths first: a host NixOS PATH doesn't resolve in the sandbox.
            ("PATH", format!("/usr/bin:/bin:/usr/sbin:/sbin:{host_path}")),
            // The 64-bit overlay library needs libGL from an FHS system, so it is only
            // preloaded inside the sandbox, never on nsenter or steam-run.
            (
                "LD_PRELOAD",
                format!(
                    "{}:{}",
                    path_str(&root.join("ubuntu12_32/gameoverlayrenderer.so")),
                    path_str(&root.join("ubuntu12_64/gameoverlayrenderer.so"))
                ),
            ),
        ]
        .into();

        let base = without_appimage(base);
        // Each with whether it was inherited.
        let mut game_env: Vec<(String, bool)> = base
            .iter()
            // TMPDIR may point outside the sandbox's private /tmp (even Steam's inherited
            // one); without it everything uses that /tmp. Secrets the user's session
            // exports stay out of untrusted game processes.
            .filter(|(k, v)| !set.contains_key(k.as_str()) && k != "TMPDIR" && !is_secret_env(k, v))
            .map(|(k, v)| (format!("{k}={v}"), true))
            .chain(set.iter().map(|(k, v)| (format!("{k}={v}"), false)))
            .collect();
        game_env.sort();
        game_env.extend(extra_env.into_iter().map(|kv| (kv, false)));

        // env sets the game's environment and working directory only once inside the
        // sandbox, where they resolve.
        let cwd = path_str(spec.exe.parent().unwrap_or(Path::new("/")));
        let mut argv: Vec<(String, bool)> =
            ["/usr/bin/env".into(), "-i".into(), format!("--chdir={cwd}")].map(|a| (a, false)).into();
        argv.extend(game_env);
        argv.extend(game.into_iter().map(|a| (a, false)));
        if let Some(wrap) = wrap {
            // Run where Steam runs, so the overlay can reach the game.
            argv.splice(0..0, wrap.into_iter().map(|a| (a, false)));
        } else if is_nixos() {
            // No Steam sandbox to join; Proton still needs an FHS system.
            if let Some(p) = look_path("steam-run") {
                argv.insert(0, (path_str(&p), false));
            }
        }
        let (argv, inherited) = argv.into_iter().unzip();
        Ok(LaunchCommand { argv, inherited })
    }

    /// Starts the game detached from this process and returns its log file. `on_exit` runs
    /// on a background thread when the game's Proton process ends.
    pub fn launch(&self, paths: &Paths, spec: &LaunchSpec, on_exit: OnExit) -> Result<PathBuf, String> {
        use std::os::unix::process::CommandExt;
        let command = self.launch_command(spec)?;
        let argv = &command.argv;
        std::fs::create_dir_all(&spec.prefix).map_err(|e| format!("creating prefix: {e}"))?;
        claim_default_prefix(paths, spec).map_err(|e| format!("claiming prefix: {e}"))?;
        let (log_path, log) = open_log(paths, &spec.name, &command).map_err(|e| format!("log: {e}"))?;
        let err_log = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]).current_dir(spec.exe.parent().unwrap_or(Path::new("/"))).stdout(log).stderr(err_log);
        // SAFETY: setsid is async-signal-safe and touches nothing in this process.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let result = match child.wait() {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(status.to_string()),
                Err(e) => Err(e.to_string()),
            };
            on_exit(result);
        });
        Ok(log_path)
    }

    /// The launch as a copy-pasteable shell line.
    pub fn shell_command(&self, spec: &LaunchSpec) -> Result<String, String> {
        Ok(self.launch_command(spec)?.shell_line())
    }
}

/// Saved settings layered over detected defaults.
pub fn build_spec(
    paths: &Paths,
    g: &Game,
    s: &crate::config::GameSettings,
    protons: &[Proton],
    default_proton: &str,
) -> Result<LaunchSpec, String> {
    let exe = match &s.exe {
        e if !e.is_empty() && Path::new(e).is_file() => PathBuf::from(e),
        _ => g.exes.first().cloned().ok_or_else(|| format!("no executable found for {}", g.name))?,
    };
    let proton_name = first_non_empty(&s.proton, default_proton);
    let proton = protons.iter().rev().find(|p| p.name == proton_name).or(protons.first()).cloned().unwrap_or_default();
    // Spacewar, what OnlineFix uses.
    let fake_app_id = [s.fake_app_id, g.fake_id].into_iter().find(|&id| id != 0).unwrap_or(480);
    let overrides = match s.overrides.as_str() {
        "" => crate::scan::dll_overrides(exe.parent().unwrap_or(Path::new("/"))),
        o => o.to_owned(),
    };
    Ok(LaunchSpec {
        name: first_non_empty(&s.name, &g.name).to_owned(),
        prefix: if s.prefix.is_empty() { default_prefix(paths, &g.root) } else { PathBuf::from(&s.prefix) },
        exe,
        game_dir: g.root.clone(),
        proton,
        fake_app_id,
        overrides,
        env: s.env.clone(),
        args: s.args.clone(),
    })
}

/// Names the game folder that launched in a prefix, so another game in a folder of the same
/// name doesn't share its saves.
const PREFIX_OWNER: &str = ".asterisk-ofix-game";

/// The Wine prefix of the game in `root`: named after its folder, as it always was, unless a
/// game in another folder of that name has claimed that prefix. Then the folder's name gets a
/// hash of its path. An owner whose folder is gone (it moved) gives the prefix up.
pub fn default_prefix(paths: &Paths, root: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let name = slug(&crate::scan::file_name(root));
    let plain = paths.data_dir().join("prefixes").join(&name);
    match prefix_owner(&plain) {
        Some(owner) if owner != root && owner.exists() => {
            // FNV-1a: stable across Rust versions, unlike std's hasher.
            let hash = root.as_os_str().as_bytes().iter().fold(0xcbf29ce484222325u64, |h, &b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            });
            paths.data_dir().join("prefixes").join(format!("{name}-{:08x}", hash as u32))
        }
        _ => plain,
    }
}

/// Records which game a launch's prefix belongs to, when it's the game's default one that
/// nobody (still around) owns. A prefix chosen by hand is left alone.
pub fn claim_default_prefix(paths: &Paths, spec: &LaunchSpec) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    if spec.prefix != default_prefix(paths, &spec.game_dir) {
        return Ok(());
    }
    if prefix_owner(&spec.prefix).is_some_and(|o| o == spec.game_dir || o.exists()) {
        return Ok(());
    }
    std::fs::create_dir_all(&spec.prefix)?;
    std::fs::write(spec.prefix.join(PREFIX_OWNER), spec.game_dir.as_os_str().as_bytes())
}

fn prefix_owner(prefix: &Path) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let data = std::fs::read(prefix.join(PREFIX_OWNER)).ok()?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&data)))
}

/// A game's log, private to the user, starting with the launch command as `shell_line` prints
/// it, and secret-looking values of our own (a per-game environment) redacted too.
pub fn open_log(paths: &Paths, name: &str, command: &LaunchCommand) -> std::io::Result<(PathBuf, std::fs::File)> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    let path = log_path(paths, name);
    let dir = path.parent().unwrap_or(Path::new("/"));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let mut f = std::fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(&path)?;
    // `mode` only applies to new files.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    let shown = command.printed(|a| match a.split_once('=') {
        Some((k, v)) if is_secret_env(k, v) => format!("{k}=[redacted]"),
        _ => shell_quote(a),
    });
    writeln!(f, "$ {shown}\n")?;
    Ok((path, f))
}

/// Where the log of the game called `name` goes; each launch overwrites it.
pub fn log_path(paths: &Paths, name: &str) -> PathBuf {
    paths.data_dir().join("logs").join(format!("{}.log", slug(name)))
}

/// Parts of variable names (split on _ and -) that mark a secret.
const SECRET_WORDS: &[&str] = &[
    "KEY",
    "KEYS",
    "APIKEY",
    "TOKEN",
    "TOKENS",
    "SECRET",
    "SECRETS",
    "PASSWORD",
    "PASSWD",
    "PASSPHRASE",
    "CREDENTIAL",
    "CREDENTIALS",
    "PAT",
];

/// How common API keys start, whatever they're called.
const SECRET_PREFIXES: &[&str] =
    &["sk-", "sk_", "ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "AKIA", "AIza"];

/// Whether an environment entry looks like a credential (or an agent socket) that a game
/// has no business seeing. OnlineFix games are untrusted Windows binaries, and the
/// environment they'd otherwise inherit is the user's whole session.
pub fn is_secret_env(name: &str, value: &str) -> bool {
    if matches!(name, "SSH_AUTH_SOCK" | "SSH_AGENT_PID" | "GPG_AGENT_INFO") {
        return true;
    }
    name.to_uppercase().split(['_', '-']).any(|part| SECRET_WORDS.contains(&part))
        || SECRET_PREFIXES.iter().any(|p| value.starts_with(p))
}

/// `env` without what the AppImage runtime adds when we run from one: its own variables, and
/// library paths inside its mount, which would shadow the libraries Proton and games expect.
fn without_appimage(env: Env) -> Env {
    let Some(appdir) = env.iter().find(|(k, _)| k == "APPDIR").map(|(_, v)| v.clone()) else { return env };
    env.into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD") && !k.starts_with("APPIMAGE_"))
        .filter_map(|(k, v)| {
            if k != "LD_LIBRARY_PATH" {
                return Some((k, v));
            }
            let kept: Vec<&str> = v.split(':').filter(|p| !p.is_empty() && !Path::new(p).starts_with(&appdir)).collect();
            (!kept.is_empty()).then(|| (k, kept.join(":")))
        })
        .collect()
}

/// Asks every process of the game in `game_dir` to quit, and kills what's left a few seconds
/// later. Returns how many processes it found.
pub fn stop_game(game_dir: &Path) -> usize {
    let pids = game_pids(game_dir);
    for &pid in &pids {
        // SAFETY: kill only sends a signal.
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
    if !pids.is_empty() {
        let game_dir = game_dir.to_owned();
        std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if game_pids(&game_dir).is_empty() {
                    return;
                }
            }
            // Looked up again, so a PID reused since is never hit.
            for pid in game_pids(&game_dir) {
                // SAFETY: as above.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        });
    }
    pids.len()
}

/// The processes carrying `GAME_MARKER` for `game_dir`.
fn game_pids(game_dir: &Path) -> Vec<i32> {
    use std::os::unix::ffi::OsStrExt;
    let mut want = format!("{GAME_MARKER}=").into_bytes();
    want.extend_from_slice(game_dir.as_os_str().as_bytes());
    let me = std::process::id() as i32;
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&pid| pid != me)
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|env| env.split(|&b| b == 0).any(|kv| kv == want))
        })
        .collect()
}

fn proc_environ(pid: i32) -> Env {
    let data = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    data.split(|&b| b == 0)
        .filter_map(|kv| {
            let kv = String::from_utf8_lossy(kv);
            kv.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .collect()
}

fn is_nixos() -> bool {
    std::fs::read_to_string("/etc/os-release")
        .is_ok_and(|s| s.lines().any(|l| matches!(l.trim(), "ID=nixos" | "ID=\"nixos\"")))
}

pub fn slug(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

/// Splits on whitespace, honouring single and double quotes and backslashes.
pub fn split_shell(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_word, mut escaped) = (false, false);
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match c {
            _ if escaped => {
                // Inside double quotes only these are escaped; other backslashes stay.
                if quote == Some('"') && !matches!(c, '$' | '`' | '"' | '\\' | '\n') {
                    cur.push('\\');
                }
                cur.push(c);
                escaped = false;
            }
            '\\' if quote != Some('\'') => {
                escaped = true;
                in_word = true;
            }
            _ if quote.is_some() => {
                if Some(c) == quote {
                    quote = None
                } else {
                    cur.push(c)
                }
            }
            '"' | '\'' => {
                quote = Some(c);
                in_word = true;
            }
            ' ' | '\t' | '\n' => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            _ => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    if escaped {
        cur.push('\\');
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && !s.contains([
            ' ', '\t', '\n', '\'', '"', '\\', '$', '`', ';', '&', '|', '<', '>', '(', ')', '*', '?', '[', ']', '#',
            '~', '!', '{', '}',
        ])
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn first_non_empty<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.is_empty() { b } else { a }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn lossy(k: std::ffi::OsString, v: std::ffi::OsString) -> (String, String) {
    (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
