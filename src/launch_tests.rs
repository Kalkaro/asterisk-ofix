use super::*;
use crate::testutil::tempdir;

fn env(pairs: &[(&str, &str)]) -> Env {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// The KEY=VALUE list given to `env -i` in a launch command.
fn game_env(argv: &[String]) -> BTreeMap<String, String> {
    let i = argv.iter().position(|a| a == "/usr/bin/env").expect("no /usr/bin/env in the command");
    assert_eq!(argv[i + 1], "-i");
    argv[i + 2..]
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map_while(|a| if a.starts_with('/') { None } else { a.split_once('=') })
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn spec() -> LaunchSpec {
    LaunchSpec {
        exe: "/g/game.exe".into(),
        game_dir: "/g".into(),
        prefix: "/pfx".into(),
        fake_app_id: 480,
        proton: Proton { dir: "/p".into(), ..Proton::default() },
        ..LaunchSpec::default()
    }
}

/// Not Steam: a PID whose namespaces can't be read, so nothing is entered.
fn no_sandbox() -> Steam {
    Steam { root: "/steam".into(), sandbox_pid: -1 }
}

#[test]
fn command_runs_proton_in_the_runtime_with_the_overlay_env() {
    let spec = LaunchSpec {
        overrides: "a=n".into(),
        proton: Proton { dir: "/p".into(), runtime: Some("/rt/_v2-entry-point".into()), ..Proton::default() },
        env: r#"DXVK_HUD="fps,memory" X=1"#.into(),
        args: r#"-windowed "two words""#.into(),
        ..spec()
    };
    let argv = no_sandbox().command_from(&spec, env(&[("PATH", "/bin")])).unwrap();
    assert!(
        argv.join(" ").ends_with(
            "/rt/_v2-entry-point --verb=waitforexitandrun -- /p/proton waitforexitandrun /g/game.exe -windowed two words"
        ),
        "{argv:?}"
    );
    assert_eq!(argv.last().unwrap(), "two words");
    let e = game_env(&argv);
    for (k, v) in [
        ("SteamGameId", "480"),
        ("WINEDLLOVERRIDES", "a=n"),
        ("STEAM_COMPAT_DATA_PATH", "/pfx"),
        ("ENABLE_VK_LAYER_VALVE_steam_overlay_1", "1"),
        ("DXVK_HUD", "fps,memory"),
        ("STEAM_COMPAT_TOOL_PATHS", "/p:/rt"),
    ] {
        assert_eq!(e.get(k).map(String::as_str), Some(v), "game env {k}");
    }
}

#[test]
fn command_rejects_bad_quoting_and_env() {
    let bad_args = LaunchSpec { args: "\"open".into(), ..spec() };
    assert!(no_sandbox().command_from(&bad_args, Env::new()).unwrap_err().contains("arguments"));
    let bad_env = LaunchSpec { env: "NOEQUALS".into(), ..spec() };
    assert!(no_sandbox().command_from(&bad_env, Env::new()).unwrap_err().contains("KEY=VALUE"));
    let no_proton = LaunchSpec { proton: Proton::default(), ..spec() };
    assert!(no_sandbox().command_from(&no_proton, Env::new()).is_err());
}

/// The command must enter Steam's sandbox first and only then, through `env -i`, set the
/// game's environment and working directory. Each of these broke a real launch on NixOS:
///   - LD_PRELOAD on nsenter itself: "nsenter: ... libGL.so.1: cannot open" (exit 127)
///   - the host cwd inside the sandbox: "getcwd: cannot access parent directories"
///   - the host NixOS PATH inside the sandbox: "bwrap: execvp true: No such file"
///   - the host TMPDIR inside the sandbox: "pv-adverb: Cannot create temporary directory"
#[test]
fn command_sets_the_game_env_only_inside_the_sandbox() {
    let spec = LaunchSpec {
        exe: "/g/bin/game.exe".into(),
        proton: Proton { dir: "/p".into(), runtime: Some("/rt/_v2-entry-point".into()), ..Proton::default() },
        ..spec()
    };
    let host = env(&[("TMPDIR", "/host/only/tmp"), ("PATH", "/run/current-system/sw/bin")]);
    let argv = no_sandbox().command_from(&spec, host).unwrap();
    let i = argv.iter().position(|a| a == "/usr/bin/env").unwrap();
    for a in &argv[..i] {
        assert!(a.ends_with("steam-run"), "unexpected {a:?} before env in {argv:?}");
    }
    assert_eq!(argv[i + 2], "--chdir=/g/bin");
    let e = game_env(&argv);
    assert_eq!(e["PATH"], "/usr/bin:/bin:/usr/sbin:/sbin:/run/current-system/sw/bin");
    assert_eq!(e["LD_PRELOAD"], "/steam/ubuntu12_32/gameoverlayrenderer.so:/steam/ubuntu12_64/gameoverlayrenderer.so");
    assert!(!e.contains_key("TMPDIR"), "the host TMPDIR leaked into the game env");
    assert!(
        argv[..i].iter().all(|a| !a.contains("gameoverlayrenderer")),
        "the overlay must only be preloaded inside env -i"
    );
}

/// Joining Steam's sandbox, the game must get Steam's environment (its DISPLAY and
/// XAUTHORITY work in there). With the launcher's own display games died with "No displays
/// available".
#[test]
fn command_uses_steams_environment() {
    if look_path("nsenter").is_none() {
        return eprintln!("skipped: nsenter not found");
    }
    let Ok(mut child) = std::process::Command::new("unshare")
        .args(["--user", "--mount", "sleep", "30"])
        .env_clear()
        .envs([("DISPLAY", ":steam"), ("TMPDIR", "/steam/tmp"), ("PATH", "/usr/bin")])
        .spawn()
    else {
        return eprintln!("skipped: can't create a namespaced process");
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    let steam = Steam { root: "/steam".into(), sandbox_pid: child.id() as i32 };
    let argv = steam.command_from(&spec(), env(&[("DISPLAY", ":launcher")]));
    let _ = child.kill();
    let _ = child.wait();
    let argv = argv.unwrap();
    if !argv[0].ends_with("nsenter") {
        return eprintln!("skipped: namespaces not distinguishable here: {:?}", &argv[..3]);
    }
    let e = game_env(&argv);
    assert_eq!(e["DISPLAY"], ":steam", "the game should get Steam's DISPLAY");
    assert!(!e.contains_key("TMPDIR"), "TMPDIR must be dropped even from Steam's env");
    assert_eq!(e["SteamGameId"], "480", "our own variables still apply");
}

#[test]
fn recognises_secrets() {
    for (kv, secret) in [
        ("CONDUIT_API_KEY=sk-cdt-abc", true),
        ("GITHUB_TOKEN=x", true),
        ("AWS_SECRET_ACCESS_KEY=x", true),
        ("DB_PASSWORD=x", true),
        ("SSH_AUTH_SOCK=/run/user/1/ssh", true),
        ("MY_THING=sk-proj-123", true), // looks like a key whatever it's called
        ("SOMETHING=ghp_abc", true),
        ("DISPLAY=:0", false),
        ("XAUTHORITY=/home/u/.Xauthority", false),
        ("WAYLAND_DISPLAY=wayland-1", false),
        ("LESSKEYIN_SYSTEM=/nix/store/x", false), // KEY inside a word isn't a key
        ("KEYBOARD_LAYOUT=us", false),
        ("PATH=/usr/bin", false),
        ("SteamAppId=480", false),
        ("WINEDLLOVERRIDES=winmm=n,b", false),
    ] {
        let (k, v) = kv.split_once('=').unwrap();
        assert_eq!(is_secret_env(k, v), secret, "{kv}");
    }
}

/// The session's secrets must never reach a game's environment, and so neither its log nor
/// the copied command. This leaked a real API key once.
#[test]
fn secrets_never_reach_the_game() {
    let host = env(&[
        ("CONDUIT_API_KEY", "sk-cdt-supersecret"),
        ("GH_TOKEN", "ghp_alsosecret"),
        ("SSH_AUTH_SOCK", "/run/user/1/ssh-agent"),
        ("DISPLAY", ":7"),
    ]);
    // Our own process shares our namespaces, so the host environment is used.
    let steam = Steam { root: "/steam".into(), sandbox_pid: std::process::id() as i32 };
    let argv = steam.command_from(&spec(), host).unwrap();
    let joined = argv.join(" ");
    for leak in ["supersecret", "alsosecret", "CONDUIT_API_KEY", "SSH_AUTH_SOCK"] {
        assert!(!joined.contains(leak), "{leak} leaked into the game command");
    }
    assert_eq!(game_env(&argv)["DISPLAY"], ":7", "ordinary variables still pass through");
}

#[test]
fn log_is_private_and_redacted() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir();
    let paths = Paths::under(home.path());
    // Logs from older versions were world-readable; reopening must fix that.
    let dir = paths.data_dir().join("logs");
    crate::testutil::write(&dir.join("Cool_Game.log"), "old");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(dir.join("Cool_Game.log"), std::fs::Permissions::from_mode(0o644)).unwrap();

    let argv: Vec<String> =
        ["/usr/bin/env", "-i", "MY_API_KEY=hunter2", "DISPLAY=:0", "/p/proton"].map(String::from).to_vec();
    let (path, f) = open_log(&paths, "Cool Game", &argv).unwrap();
    drop(f);
    let data = std::fs::read_to_string(&path).unwrap();
    assert!(!data.contains("hunter2") && data.contains("MY_API_KEY=[redacted]"), "{data}");
    assert!(data.contains("DISPLAY=:0"), "ordinary variables are still logged");
    for (p, want) in [(&path, 0o600), (&dir, 0o700)] {
        assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, want, "{}", p.display());
    }
}

#[test]
fn splits_and_quotes_like_a_shell() {
    assert_eq!(split_shell(r#"a "b c" 'd\e' f\ g"#).unwrap(), ["a", "b c", r"d\e", "f g"]);
    assert_eq!(split_shell("  ").unwrap(), Vec::<String>::new());
    assert_eq!(split_shell("''").unwrap(), [""]);
    // In double quotes a backslash only escapes $ ` " \ and newlines, as in sh, so Windows
    // paths survive. They used to lose every backslash: "C:\Games" became C:Games.
    assert_eq!(split_shell(r#"-path "C:\Games\x""#).unwrap(), ["-path", r"C:\Games\x"]);
    assert_eq!(split_shell(r#""a\"b\\c\$d""#).unwrap(), [r#"a"b\c$d"#]);
    assert_eq!(split_shell(r"trailing\").unwrap(), [r"trailing\"]);
    assert_eq!(shell_join(&["a b".into(), "it's".into(), "plain".into()]), r"'a b' 'it'\''s' plain");
    assert_eq!(slug("Cool Game: 2"), "Cool_Game__2");
}

/// Prefixes were named after the game's folder alone, so two games in folders of the same
/// name shared saves: hand-picked Unreal games (both in Binaries/Win64), or one folder name
/// in two libraries. The first to launch keeps the plain name; the other gets its own.
#[test]
fn games_in_same_named_folders_get_their_own_prefixes() {
    let home = tempdir();
    let paths = Paths::under(home.path());
    let a = home.path().join("A/Binaries/Win64");
    let b = home.path().join("B/Binaries/Win64");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let plain = paths.data_dir().join("prefixes/Win64");
    assert_eq!(default_prefix(&paths, &a), plain, "an unclaimed prefix keeps the old name");

    claim_default_prefix(&paths, &LaunchSpec { prefix: plain.clone(), game_dir: a.clone(), ..spec() }).unwrap();
    assert_eq!(default_prefix(&paths, &a), plain, "the game that launched keeps it");
    let own = default_prefix(&paths, &b);
    assert_ne!(own, plain, "another game with the same folder name doesn't share it");
    assert!(crate::scan::file_name(&own).starts_with("Win64-"), "{}", own.display());
    assert_eq!(default_prefix(&paths, &b), own, "and its name is stable");

    // A game whose folder moved finds its saves again: the old owner is gone.
    let moved = home.path().join("Elsewhere/A/Binaries/Win64");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::remove_dir_all(home.path().join("A")).unwrap();
    assert_eq!(default_prefix(&paths, &moved), plain);
}

/// A prefix the user picked by hand (or with --prefix) is never claimed, or launching one
/// game in another's prefix would take that prefix from it.
#[test]
fn a_chosen_prefix_is_not_claimed() {
    let home = tempdir();
    let paths = Paths::under(home.path());
    let (a, b) = (home.path().join("Game"), home.path().join("Other"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let plain = default_prefix(&paths, &a);
    claim_default_prefix(&paths, &LaunchSpec { prefix: plain.clone(), game_dir: b, ..spec() }).unwrap();
    assert_eq!(default_prefix(&paths, &a), plain, "b launched in a prefix that isn't its default");
}

#[test]
fn settings_override_detected_defaults() {
    let home = tempdir();
    let paths = Paths::under(home.path());
    let root = crate::testutil::add_game(&home.path().join("Games"), "Pong", 0);
    let game = crate::scan::detect_game(&root).unwrap();
    let protons = vec![
        Proton { name: "GE-Proton10-4".into(), dir: "/a".into(), runtime: None },
        Proton { name: "Proton 9.0".into(), dir: "/b".into(), runtime: None },
    ];
    let detected = build_spec(&paths, &game, &Default::default(), &protons, "Proton 9.0").unwrap();
    assert_eq!(detected.exe, root.join("Pong.exe"));
    assert_eq!(detected.proton.name, "Proton 9.0");
    assert_eq!(detected.fake_app_id, 480);
    assert_eq!(detected.overrides, "SteamFix64=n;steam_api64=n");
    assert_eq!(detected.prefix, paths.data_dir().join("prefixes/Pong"));

    let s = crate::config::GameSettings {
        proton: "GE-Proton10-4".into(),
        fake_app_id: 1234,
        overrides: "x=n".into(),
        exe: "/missing.exe".into(), // gone: falls back to the detected one
        ..Default::default()
    };
    let spec = build_spec(&paths, &game, &s, &protons, "Proton 9.0").unwrap();
    assert_eq!((spec.proton.name.as_str(), spec.fake_app_id, spec.overrides.as_str()), ("GE-Proton10-4", 1234, "x=n"));
    assert_eq!(spec.exe, root.join("Pong.exe"));
}
