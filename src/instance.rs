//! One window per user. Each window kept its own copy of config.json and saved all of it, so
//! two of them undid each other's changes.
//!
//! The window holds an flock on `window.lock` in the runtime dir for as long as it runs; the
//! kernel lets go of it when the process ends, crashed or not, so nothing stale can keep the
//! app from starting. It listens on `window.sock` beside it. A later launch finds the lock
//! taken, sends its activation token down the socket and exits, and the first window comes
//! forward with that token (Wayland lets a window take focus only with one). Headless runs and
//! the CLI never take the lock.

use crate::paths::Paths;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// What a later launch sends, before its activation token.
const SHOW: &str = "show";
/// The longest message read from a later launch.
const MAX_MESSAGE: u64 = 4096;

pub enum Claim {
    /// This is the window.
    First(Instance),
    /// Another process holds the window's lock.
    Taken,
}

/// The window's lock and socket, held while it runs.
pub struct Instance {
    /// Locked; closing it unlocks. Rust opens files close-on-exec, so games and installers
    /// launched from the window don't inherit it and can't keep the lock past its exit.
    _lock: File,
    listener: Option<UnixListener>,
}

/// Takes the window's lock, or finds another process holding it.
pub fn claim(paths: &Paths) -> std::io::Result<Claim> {
    let dir = paths.runtime_dir();
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(dir.join("window.lock"))?;
    // SAFETY: flock on a descriptor this function owns.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::EWOULDBLOCK) => Ok(Claim::Taken),
            _ => Err(e),
        };
    }
    // A socket left by a window that's gone; nobody else binds it while we hold the lock.
    let socket = socket_path(paths);
    let _ = std::fs::remove_file(&socket);
    // Without the socket the window still runs alone; later launches just can't raise it.
    let listener = UnixListener::bind(&socket).ok();
    Ok(Claim::First(Instance { _lock: lock, listener }))
}

impl Instance {
    /// Hands the activation token of every later launch to `show`, on a thread of its own
    /// that holds the lock until the process exits.
    pub fn serve(self, show: impl Fn(String) + Send + 'static) {
        std::thread::Builder::new()
            .name("instance".into())
            .spawn(move || {
                let Instance { _lock, listener } = self;
                let Some(listener) = listener else { return std::mem::forget(_lock) };
                for stream in listener.incoming().flatten() {
                    if let Some(token) = read_request(stream) {
                        show(token);
                    }
                }
            })
            .expect("spawning the instance thread");
    }
}

/// The token a `show` request carries ("" without one), or None for anything else.
fn read_request(stream: UnixStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    let mut line = String::new();
    BufReader::new(std::io::Read::take(stream, MAX_MESSAGE)).read_line(&mut line).ok()?;
    let rest = line.trim_end().strip_prefix(SHOW)?;
    Some(rest.trim().to_owned())
}

/// Asks the window to come forward with `token` (`XDG_ACTIVATION_TOKEN`, or
/// `DESKTOP_STARTUP_ID` on X11). A window that just took its lock may not be listening yet,
/// so this tries for a couple of seconds.
pub fn notify(paths: &Paths, token: &str) -> std::io::Result<()> {
    let token: String = token.chars().filter(|c| !c.is_control()).collect();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match UnixStream::connect(socket_path(paths)) {
            Ok(mut stream) => return writeln!(stream, "{SHOW} {token}"),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn socket_path(paths: &Paths) -> PathBuf {
    paths.runtime_dir().join("window.sock")
}

/// The token the desktop gave this launch, which lets the window it reaches take focus.
pub fn activation_token(env: impl Fn(&str) -> Option<String>) -> String {
    env("XDG_ACTIVATION_TOKEN").or_else(|| env("DESKTOP_STARTUP_ID")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;
    use std::sync::mpsc;

    fn first(paths: &Paths) -> Instance {
        match claim(paths).unwrap() {
            Claim::First(i) => i,
            Claim::Taken => panic!("the lock was taken"),
        }
    }

    /// The lock is per open file, so a second claim in the same process stands in for a
    /// second launch.
    #[test]
    fn a_second_window_finds_the_first() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let window = first(&paths);
        assert!(matches!(claim(&paths).unwrap(), Claim::Taken));
        // The kernel lets go when the holder goes, as it does when a process dies.
        drop(window);
        let again = first(&paths);
        assert!(matches!(claim(&paths).unwrap(), Claim::Taken));
        drop(again);
    }

    #[test]
    fn a_second_launch_raises_the_first_with_its_token() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let (tx, rx) = mpsc::channel();
        first(&paths).serve(move |token| drop(tx.send(token)));
        notify(&paths, "abc-123\n").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "abc-123");
        notify(&paths, "").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "");
        // Anything else on the socket is ignored.
        writeln!(UnixStream::connect(socket_path(&paths)).unwrap(), "quit").unwrap();
        notify(&paths, "next").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "next");
    }

    /// A game launched from the window would otherwise hold the lock after the window closed,
    /// and asterofix wouldn't start again until the game ended.
    #[test]
    fn games_never_inherit_the_lock() {
        let home = tempdir();
        let window = first(&Paths::under(home.path()));
        // SAFETY: reads the flags of a descriptor `window` owns.
        let flags = unsafe { libc::fcntl(window._lock.as_raw_fd(), libc::F_GETFD) };
        assert!(flags & libc::FD_CLOEXEC != 0, "the lock is inherited by child processes");
        let socket = window.listener.as_ref().expect("a socket").as_raw_fd();
        // SAFETY: as above.
        assert!(unsafe { libc::fcntl(socket, libc::F_GETFD) } & libc::FD_CLOEXEC != 0);
    }

    #[test]
    fn the_token_comes_from_the_launch() {
        let env = |vars: &'static [(&str, &str)]| {
            move |k: &str| vars.iter().find(|(name, _)| *name == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(activation_token(env(&[("XDG_ACTIVATION_TOKEN", "w"), ("DESKTOP_STARTUP_ID", "x")])), "w");
        assert_eq!(activation_token(env(&[("DESKTOP_STARTUP_ID", "x")])), "x");
        assert_eq!(activation_token(env(&[])), "");
    }
}
