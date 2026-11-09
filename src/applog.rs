//! asterofix's own log, for bug reports: what it found at startup, every message it showed,
//! the games it launched and how they ended, and crashes. It's `asterofix.log` in the data
//! dir, private to the user, and the previous run's is kept as `asterofix.old.log`. Pressing
//! the logo copies its end, with the system's details, as diagnostics (`App::diagnostics`).

use crate::paths::Paths;
use std::fmt::Display;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A run writes this much at most, so a loop of errors can't fill the disk.
const MAX_BYTES: u64 = 2 << 20;

pub struct AppLog {
    path: PathBuf,
    /// The open file and how much this run wrote to it; None when it couldn't be opened, and
    /// then nothing is logged.
    file: Mutex<Option<(std::fs::File, u64)>>,
}

impl AppLog {
    /// Starts this run's log, keeping the last run's as `asterofix.old.log`.
    pub fn open(paths: &Paths) -> AppLog {
        let path = paths.data_dir().join("asterofix.log");
        let log = AppLog { path, file: Mutex::new(None) };
        *log.file.lock().unwrap_or_else(|e| e.into_inner()) = start(&log.path, &log.old_path()).ok().map(|f| (f, 0));
        log
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The last run's log.
    pub fn old_path(&self) -> PathBuf {
        self.path.with_file_name("asterofix.old.log")
    }

    /// Adds a line, stamped with the time. Lines after the first of a message are indented.
    pub fn line(&self, message: impl Display) {
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let Some((file, written)) = guard.as_mut() else { return };
        if *written >= MAX_BYTES {
            return;
        }
        let mut text = format!("{} {}\n", timestamp(SystemTime::now()), message.to_string().replace('\n', "\n    "));
        if *written + text.len() as u64 >= MAX_BYTES {
            text = format!("{} (log full; nothing more is written this run)\n", timestamp(SystemTime::now()));
            *written = MAX_BYTES;
        } else {
            *written += text.len() as u64;
        }
        let _ = file.write_all(text.as_bytes());
    }
}

/// Moves the last run's log aside and creates this run's, readable only by the user.
fn start(path: &Path, old: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("/")))?;
    if path.exists() {
        std::fs::rename(path, old)?;
        std::fs::set_permissions(old, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path)
}

/// Logs a panic before the default hook prints it: started from the app menu, nobody sees
/// stderr, so a crash otherwise leaves nothing behind.
pub fn log_panics(log: std::sync::Arc<AppLog>) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
        let backtrace = std::backtrace::Backtrace::force_capture();
        log.line(format!("crashed in thread {thread}: {info}\n{backtrace}"));
        default(info);
    }));
}

/// The last `n` lines of a file, if it can be read.
pub fn tail(path: &Path, n: usize) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    Some(lines[lines.len().saturating_sub(n)..].join("\n"))
}

/// `2026-11-09 21:06:54 UTC`.
pub fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The machine, for the log and for diagnostics. Read once at startup from the environment
/// and the system's files; tests build one from a temp root.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    /// `PRETTY_NAME` from os-release.
    pub os: String,
    pub kernel: String,
    /// Wayland or X11, and the desktop.
    pub session: String,
    /// Running as the AppImage.
    pub appimage: bool,
}

impl Host {
    /// `root` is `/`, except in tests; `env` reads the environment.
    pub fn detect(root: &Path, env: impl Fn(&str) -> Option<String>) -> Host {
        let os_release = std::fs::read_to_string(root.join("etc/os-release"))
            .or_else(|_| std::fs::read_to_string(root.join("usr/lib/os-release")))
            .unwrap_or_default();
        let os = os_release
            .lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim().trim_matches('"').to_owned())
            .unwrap_or_else(|| "unknown".into());
        let kernel = std::fs::read_to_string(root.join("proc/sys/kernel/osrelease"))
            .map(|k| k.trim().to_owned())
            .unwrap_or_else(|_| "unknown".into());
        let set = |k: &str| env(k).filter(|v| !v.is_empty());
        let kind = set("XDG_SESSION_TYPE").unwrap_or_else(|| {
            if set("WAYLAND_DISPLAY").is_some() {
                "wayland".into()
            } else if set("DISPLAY").is_some() {
                "x11".into()
            } else {
                "unknown".into()
            }
        });
        let session = match set("XDG_CURRENT_DESKTOP") {
            Some(desktop) => format!("{kind}, {desktop}"),
            None => kind,
        };
        Host { os, kernel, session, appimage: set("APPIMAGE").is_some() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{tempdir, write};
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00 UTC");
        let t = UNIX_EPOCH + Duration::from_secs(1_793_049_414);
        assert_eq!(timestamp(t), "2026-10-26 21:16:54 UTC");
        // A leap day.
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29 00:00:00 UTC");
    }

    /// Each run starts a fresh log, private to the user, and keeps the one before.
    #[test]
    fn log_is_private_and_keeps_the_last_run() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempdir();
        let paths = Paths::under(home.path());
        let first = AppLog::open(&paths);
        first.line("first run");
        first.line("two\nlines");
        drop(first);
        let log = AppLog::open(&paths);
        log.line("second run");
        let now = std::fs::read_to_string(log.path()).unwrap();
        let old = std::fs::read_to_string(log.old_path()).unwrap();
        assert!(now.contains("second run") && !now.contains("first run"), "{now}");
        assert!(old.contains("first run") && old.contains("UTC two\n    lines\n"), "{old}");
        for p in [log.path(), &log.old_path()] {
            assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600, "{}", p.display());
        }
        assert_eq!(tail(log.path(), 1).unwrap(), now.trim_end());
    }

    #[test]
    fn a_run_stops_logging_when_its_log_is_full() {
        let home = tempdir();
        let log = AppLog::open(&Paths::under(home.path()));
        let line = "x".repeat(4096);
        for _ in 0..(MAX_BYTES / 4096 + 10) {
            log.line(&line);
        }
        let size = std::fs::metadata(log.path()).unwrap().len();
        assert!(size <= MAX_BYTES + 200, "{size}");
        assert!(tail(log.path(), 1).unwrap().contains("log full"));
    }

    #[test]
    fn host_is_read_from_the_system() {
        let root = tempdir();
        write(&root.path().join("etc/os-release"), "NAME=Arch\nPRETTY_NAME=\"Arch Linux\"\n");
        write(&root.path().join("proc/sys/kernel/osrelease"), "6.12.1-arch1-1\n");
        let env = |vars: &'static [(&str, &str)]| {
            move |k: &str| vars.iter().find(|(name, _)| *name == k).map(|(_, v)| v.to_string())
        };
        let host = Host::detect(
            root.path(),
            env(&[("XDG_SESSION_TYPE", "wayland"), ("XDG_CURRENT_DESKTOP", "Hyprland"), ("APPIMAGE", "/a.AppImage")]),
        );
        assert_eq!(
            host,
            Host {
                os: "Arch Linux".into(),
                kernel: "6.12.1-arch1-1".into(),
                session: "wayland, Hyprland".into(),
                appimage: true
            }
        );
        let bare = Host::detect(&root.path().join("nothing"), env(&[("DISPLAY", ":0")]));
        assert_eq!((bare.os.as_str(), bare.session.as_str(), bare.appimage), ("unknown", "x11", false));
    }
}
