//! A real Windows program through the whole launch path (nsenter, env, Steam Linux Runtime,
//! Proton) inside a fake "Steam" sandbox: a steam-run process, which like NixOS Steam has a
//! private /tmp. It checks that the game
//!   - starts and exits cleanly,
//!   - sees the sandbox's /tmp (where the overlay exchanges frames),
//!   - has the overlay library loaded and the overlay env set,
//!   - inherits Steam's environment.
//!
//! It needs Steam, a Proton build with a runtime, steam-run and nsenter, and takes a minute
//! the first time (Wine prefix creation):
//!
//!     ASTERISK_OFIX_INTEGRATION=1 nix develop -c cargo test sandbox -- --nocapture

use crate::launch::LaunchSpec;
use crate::paths::Paths;
use crate::steam::look_path;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn launch_inside_steam_sandbox() {
    if std::env::var_os("ASTERISK_OFIX_INTEGRATION").is_none() {
        return eprintln!("skipped: set ASTERISK_OFIX_INTEGRATION=1 to run");
    }
    let real = Paths::from_env();
    let Some(mut steam) = crate::steam::detect(&real.home) else { return eprintln!("skipped: no Steam install") };
    let Some(proton) = steam.protons().into_iter().find(|p| p.runtime.is_some()) else {
        return eprintln!("skipped: no Proton build with a Steam Linux Runtime");
    };
    for tool in ["steam-run", "nsenter"] {
        if look_path(tool).is_none() {
            return eprintln!("skipped: {tool} not found");
        }
    }

    // Everything the game touches is under $HOME: the sandbox can't see this process's /tmp.
    let base = real.home.join(".cache/asterisk-ofix-test");
    let game_dir = base.join("game");
    let prefix = base.join("prefix"); // kept between runs; creating it is slow
    std::fs::create_dir_all(&game_dir).unwrap();
    let _cleanup = Cleanup(game_dir.clone());
    let paths = Paths::under(&base.join("home"));

    let exe = game_dir.join("game.exe");
    let Ok(cmd) = std::fs::read(proton.dir.join("files/lib/wine/x86_64-windows/cmd.exe")) else {
        return eprintln!("skipped: Proton's cmd.exe missing");
    };
    std::fs::write(&exe, cmd).unwrap();

    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let token = format!("asterisk-ofix{}x{nanos}", std::process::id());
    let marker = format!("/tmp/{token}");

    // The fake Steam: writes a marker into its private /tmp and idles.
    let mut fake = std::process::Command::new("steam-run")
        .args(["sh", "-c", &format!("echo seen-{token} > {marker} && exec sleep 600")])
        .env("ASTERISK_OFIX_FAKE_STEAM", &token)
        .spawn()
        .unwrap();
    let sandbox_pid = wait_for_pid(&format!("ASTERISK_OFIX_FAKE_STEAM={token}"), "sleep", Duration::from_secs(20));
    if Path::new(&marker).exists() {
        eprintln!("note: the fake sandbox's /tmp is not private here; the /tmp check proves less");
    }

    steam.sandbox_pid = sandbox_pid;
    let win = |p: &Path| format!("Z:{}", p.to_string_lossy().replace('/', "\\"));
    let spec = LaunchSpec {
        name: "asterisk-ofix-integration".into(),
        exe: exe.clone(),
        game_dir: game_dir.clone(),
        proton,
        prefix,
        fake_app_id: 480,
        overrides: "winmm=n,b".into(),
        env: format!("ASTERISK_OFIX_TEST_TOKEN={token}"),
        // Proton doesn't forward console output, so the probe reports through files in the
        // game folder (Z: is the Unix root), then stays alive so we can inspect it.
        args: format!(
            r"/c 'echo started> {gd}\started.txt & copy Z:\tmp\{token} {gd}\seen.txt & ping -n 8 127.0.0.1 >nul'",
            gd = win(&game_dir)
        ),
    };
    let argv = steam.command(&spec).unwrap();
    assert!(argv[0].ends_with("nsenter"), "the launch should enter the sandbox: {argv:?}");

    let (tx, rx) = std::sync::mpsc::channel();
    let log_path = steam.launch(&paths, &spec, Box::new(move |r| drop(tx.send(r)))).unwrap();

    // While it runs, find a Wine process of ours and check the overlay hooks.
    let (mut overlay_loaded, mut env_ok, mut steam_env) = (false, false, false);
    let deadline = Instant::now() + Duration::from_secs(240);
    let exit = loop {
        if let Ok(r) = rx.recv_timeout(Duration::from_millis(300)) {
            break Some(r);
        }
        if Instant::now() > deadline {
            break None;
        }
        for pid in pids_with_env(&format!("ASTERISK_OFIX_TEST_TOKEN={token}")) {
            let maps = std::fs::read_to_string(format!("/proc/{pid}/maps")).unwrap_or_default();
            overlay_loaded |= maps.contains("ubuntu12_64/gameoverlayrenderer.so");
            let env = environ(pid);
            let get = |k: &str| env.get(k).map(String::as_str);
            env_ok |= get("SteamGameId") == Some("480")
                && get("ENABLE_VK_LAYER_VALVE_steam_overlay_1") == Some("1")
                && get("WINEDLLOVERRIDES") == Some("winmm=n,b");
            // Inherited from the fake Steam process, as DISPLAY would be.
            steam_env |= get("ASTERISK_OFIX_FAKE_STEAM") == Some(token.as_str());
        }
    };
    let _ = fake.kill();
    let _ = fake.wait();

    let log = tail(&std::fs::read_to_string(&log_path).unwrap_or_default());
    let exit = exit.unwrap_or_else(|| panic!("the game didn't exit within 4 minutes; log:\n{log}"));
    assert!(exit.is_ok(), "the game exited with {exit:?}; log:\n{log}");
    // Known ways the launch broke on NixOS.
    for bad in ["libGL.so.1", "getcwd", "execvp", "Cannot create temporary directory"] {
        assert!(!log.contains(bad), "log contains {bad:?}:\n{log}");
    }
    assert!(game_dir.join("started.txt").exists(), "the probe program never ran; log:\n{log}");
    let seen = std::fs::read_to_string(game_dir.join("seen.txt")).unwrap_or_default();
    assert!(seen.contains(&format!("seen-{token}")), "the game can't see the Steam sandbox's /tmp; log:\n{log}");
    assert!(overlay_loaded, "gameoverlayrenderer.so was not loaded in any game process");
    assert!(steam_env, "the game didn't inherit Steam's environment (DISPLAY and co. come from there)");
    assert!(env_ok, "no game process had the Steam IDs, overlay layer and DLL overrides set");
}

struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait_for_pid(env_kv: &str, comm: &str, timeout: Duration) -> i32 {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Some(pid) = pids_with_env(env_kv)
            .into_iter()
            .find(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == comm))
        {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("no {comm} process with {env_kv} appeared");
}

fn pids_with_env(env_kv: &str) -> Vec<i32> {
    let needle = format!("\0{env_kv}\0");
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .filter(|pid: &i32| {
            std::fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|d| format!("\0{}", String::from_utf8_lossy(&d)).contains(&needle))
        })
        .collect()
}

fn environ(pid: i32) -> HashMap<String, String> {
    let data = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    String::from_utf8_lossy(&data)
        .split('\0')
        .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())))
        .collect()
}

fn tail(log: &str) -> String {
    let keep: Vec<&str> =
        log.lines().filter(|l| !l.contains("ELFCLASS32") && !l.contains("fork without exec")).collect();
    keep[keep.len().saturating_sub(25)..].join("\n")
}
