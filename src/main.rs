//! asterofix launches Steam Fix / Online Fix games directly through Proton with the Steam overlay attached,
//! without adding them to Steam.

mod applog;
mod art;
mod background;
mod cli;
mod config;
mod install;
mod instance;
#[cfg(test)]
mod integration_tests;
mod launch;
mod paths;
mod prefixes;
mod scan;
mod steam;
#[cfg(test)]
mod testutil;
mod ui;
mod uninstall;
mod vdf;

use std::path::{Path, PathBuf};

fn main() {
    let result = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Window) => return ui::run(),
        Ok(cli::Command::Help) => return println!("{}", cli::USAGE),
        Ok(cli::Command::Version) => return println!("asterofix {}", applog::VERSION),
        Ok(cli::Command::Launch { game, prefix, print }) => launch_cli(&game, prefix, print),
        Ok(cli::Command::Install(files)) => install_cli(&files),
        Err(e) => {
            eprintln!("asterofix: {e}\n\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("asterofix: {e}");
        std::process::exit(1);
    }
}

fn launch_cli(target: &Path, prefix: Option<PathBuf>, print: bool) -> Result<(), String> {
    let paths = paths::Paths::from_env();
    let steam = steam::detect(&paths.home).ok_or_else(|| steam::no_steam_error(&paths.home))?;
    let target = std::path::absolute(target).map_err(|e| e.to_string())?;
    let game = if target.is_dir() {
        scan::detect_game(&target)
            .ok_or_else(|| format!("no Steam Fix / Online Fix files found in {}", target.display()))?
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

fn install_cli(files: &[PathBuf]) -> Result<(), String> {
    let paths = paths::Paths::from_env();
    let steam = steam::detect(&paths.home).ok_or_else(|| steam::no_steam_error(&paths.home))?;
    let files: Vec<PathBuf> =
        files.iter().map(std::path::absolute).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    let plan = install::plan(&paths, &files)?;
    let protons = steam.protons();
    let default = steam.default_proton(&protons);
    let proton = protons.iter().find(|p| p.name == default).ok_or("no Proton build found")?;
    for (i, step) in plan.steps.iter().enumerate() {
        let spec = plan.spec(&paths, i, proton);
        let (tx, rx) = std::sync::mpsc::channel();
        let log = steam.launch(&paths, &spec, Box::new(move |r| drop(tx.send(r))))?;
        println!("installing {} into {} (log: {})", scan::file_name(&step.exe), plan.dest.display(), log.display());
        let result = rx.recv().map_err(|e| e.to_string())?.map_err(|e| format!("{}: {e}", scan::file_name(&step.exe)));
        if let Err(e) = result.and_then(|()| plan.check(i)) {
            // What the game's installer left would stop the next try; a failed fix leaves the
            // game alone.
            if !step.is_fix {
                match plan.remove_partial(&paths, &launch::game_running) {
                    Ok(true) => eprintln!("removed the unfinished install in {}", plan.dest.display()),
                    Ok(false) => {}
                    Err(cleanup) => eprintln!("warning: {cleanup}"),
                }
            }
            return Err(e);
        }
    }
    let library = plan.dest.parent().unwrap_or(&plan.dest).to_path_buf();
    // Merged into config.json as it is now, so an open window's changes stay.
    let warning = config::Config::add_saved_library(&paths, library).map_err(|e| format!("saving config: {e}"))?;
    if let Some(warning) = warning {
        eprintln!("warning: {warning}");
    }
    println!("installed {} in {}", plan.name, plan.dest.display());
    Ok(())
}
