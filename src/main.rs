//! asterisk-ofix launches OnlineFix games directly through Proton with the Steam overlay attached,
//! without adding them to Steam.

mod art;
mod background;
mod config;
#[cfg(test)]
mod integration_tests;
mod launch;
mod paths;
mod scan;
mod steam;
#[cfg(test)]
mod testutil;
mod ui;
mod vdf;

use std::path::{Path, PathBuf};

const USAGE: &str = "usage: asterisk-ofix [--launch GAME [--print] [--prefix DIR]]

  --launch GAME   launch this game's .exe (or game folder) without the window, and wait for it
  --print         with --launch: print the command instead of running it
  --prefix DIR    with --launch: the Wine prefix to use";

fn main() {
    let mut launch: Option<PathBuf> = None;
    let mut prefix: Option<PathBuf> = None;
    let mut print = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--launch" | "-launch" => launch = args.next().map(PathBuf::from),
            "--prefix" | "-prefix" => prefix = args.next().map(PathBuf::from),
            "--print" | "-print" => print = true,
            "-h" | "--help" => return println!("{USAGE}"),
            // Everything else is Rune's (headless --snap, --tree, ...).
            _ => {}
        }
    }
    let Some(target) = launch else { return ui::run() };
    if let Err(e) = launch_cli(&target, prefix, print) {
        eprintln!("asterisk-ofix: {e}");
        std::process::exit(1);
    }
}

fn launch_cli(target: &Path, prefix: Option<PathBuf>, print: bool) -> Result<(), String> {
    let paths = paths::Paths::from_env();
    let steam = steam::detect(&paths.home).ok_or("Steam installation not found")?;
    let target = std::path::absolute(target).map_err(|e| e.to_string())?;
    let game = if target.is_dir() {
        scan::detect_game(&target).ok_or_else(|| format!("no OnlineFix files found in {}", target.display()))?
    } else {
        scan::game_from_exe(&target)
    };
    let (cfg, warning) = config::Config::open(&paths);
    if let Some(warning) = warning {
        eprintln!("warning: {warning}");
    }
    let protons = steam.protons();
    let mut spec =
        launch::build_spec(&paths, &game, &cfg.settings(&game.root), &protons, &steam.default_proton(&protons))?;
    if let Some(prefix) = prefix {
        spec.prefix = prefix;
    }
    if print {
        println!("{}", steam.shell_command(&spec)?);
        return Ok(());
    }
    if !steam::steam_running() {
        eprintln!("warning: Steam is not running; the overlay and online play need it");
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let log = steam.launch(&paths, &spec, Box::new(move |r| drop(tx.send(r))))?;
    println!("launched {} with {} (log: {})", spec.name, spec.proton.name, log.display());
    rx.recv().map_err(|e| e.to_string())?
}
