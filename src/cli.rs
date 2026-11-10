//! The command line: asterofix's own flags, checked strictly, beside Rune's, which are let
//! through for Rune to read (headless `--snap` and the like).

use std::path::PathBuf;

pub const USAGE: &str = "usage: asterofix [--launch GAME [--print] [--prefix DIR]] [--install SETUP [FIX...]]

  --launch GAME   launch this game's .exe (or game folder) without the window, and wait for it
  --print         with --launch: print the command instead of running it
  --prefix DIR    with --launch: the Wine prefix to use
  --install EXE.. install a game silently into ~/Games/asterofix from its installer, then its
                  online fix (or just a fix, into the game installed before)
  -V, --version   print the version
  -h, --help      print this

Without --launch or --install it opens the window, which also takes Rune's headless flags
(--snap OUT.png, --scene NAME, --size WxH, ...).";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
    /// The window (with whatever of Rune's flags came along).
    Window,
    Help,
    Version,
    Launch {
        game: PathBuf,
        prefix: Option<PathBuf>,
        print: bool,
    },
    Install(Vec<PathBuf>),
}

/// Rune's flags that take a value, as `rune::tooling::Args` parses them (its
/// crates/rune/src/tooling/args.rs, at the rev in Cargo.toml). The value is taken whatever it
/// looks like, as Rune does. `--snap-on-patch` and `--hot-control` need Rune's `hot` feature,
/// and without it Rune reports them itself.
const RUNE_VALUE_FLAGS: &[&str] = &[
    "--snap",
    "--scene",
    "--variant",
    "--at",
    "--size",
    "--scale",
    "--theme",
    "--scroll-bars",
    "--script",
    "--frames",
    "--start",
    "--fps",
    "--out",
    "--video",
    "--duration",
    "--keys-theme",
    "--desktop",
    "--tree",
    "--html",
    "--html-template",
    "--html-widths",
    "--trace",
    "--strip",
    "--crop",
    "--cols",
    "--every",
    "--window",
    "--snap-on-patch",
    "--hot-control",
];

/// Rune's flags that take no value.
const RUNE_SWITCHES: &[&str] = &[
    "--hot",
    "--whole-window",
    "--pointer",
    "--native",
    "--trace-all",
    "--list-scenes",
    "--first-pixels",
    "--list-windows",
    "--chrome",
];

/// `--keys [CORNER]`: the corners Rune takes after it (`keycaps::Corner::parse`).
const KEYS_CORNERS: &[&str] = &["bottom-left", "bottom", "bottom-right", "top-left", "top", "top-right"];

/// Parses the arguments (without the program name). Help and version win as soon as they're
/// seen; anything not ours or Rune's is an error.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut launch: Option<PathBuf> = None;
    let mut prefix: Option<PathBuf> = None;
    let mut print = false;
    let mut install: Option<Vec<PathBuf>> = None;
    let mut args = args.into_iter().peekable();
    // A value of ours is anything but a flag; a path starting with "-" can be written "./-x".
    let is_value = |a: &String| !a.starts_with('-');
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--install" | "-install" => {
                let files = install.get_or_insert_default();
                let before = files.len();
                while let Some(exe) = args.next_if(is_value) {
                    files.push(PathBuf::from(exe));
                }
                if files.len() == before {
                    return Err(format!("{a} needs an installer or a fix"));
                }
            }
            "--launch" | "-launch" => {
                launch = Some(args.next_if(is_value).ok_or(format!("{a} needs a game"))?.into());
            }
            "--prefix" | "-prefix" => {
                prefix = Some(args.next_if(is_value).ok_or(format!("{a} needs a directory"))?.into());
            }
            "--print" | "-print" => print = true,
            f if RUNE_VALUE_FLAGS.contains(&f) => {
                args.next().ok_or(format!("{f} needs a value"))?;
            }
            f if RUNE_SWITCHES.contains(&f) => {}
            "--keys" => drop(args.next_if(|v| KEYS_CORNERS.contains(&v.as_str()))),
            // `--playground [NAME]`, a name being anything but a long flag.
            "--playground" => drop(args.next_if(|v| !v.starts_with("--"))),
            f if f.starts_with('-') => return Err(format!("unknown option {f}")),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    match (launch, install) {
        (Some(_), Some(_)) => Err("--launch and --install don't go together".into()),
        (None, Some(files)) if prefix.is_none() && !print => Ok(Command::Install(files)),
        (Some(game), None) => Ok(Command::Launch { game, prefix, print }),
        _ if prefix.is_some() => Err("--prefix needs --launch".into()),
        _ if print => Err("--print needs --launch".into()),
        _ => Ok(Command::Window),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &str) -> Result<Command, String> {
        parse(args.split_whitespace().map(String::from))
    }

    #[test]
    fn no_arguments_open_the_window() {
        assert_eq!(parse_str(""), Ok(Command::Window));
    }

    #[test]
    fn help_and_version() {
        for a in ["-h", "--help", "--launch x -h", "--print --help"] {
            assert_eq!(parse_str(a), Ok(Command::Help), "{a}");
        }
        for a in ["-V", "--version", "--snap a.png --version"] {
            assert_eq!(parse_str(a), Ok(Command::Version), "{a}");
        }
    }

    #[test]
    fn launch_takes_a_game_and_its_options() {
        assert_eq!(
            parse_str("--launch /g/Game"),
            Ok(Command::Launch { game: "/g/Game".into(), prefix: None, print: false })
        );
        assert_eq!(
            parse_str("--print --launch game.exe --prefix /p"),
            Ok(Command::Launch { game: "game.exe".into(), prefix: Some("/p".into()), print: true })
        );
        assert_eq!(
            parse_str("-launch game.exe -print"),
            Ok(Command::Launch { game: "game.exe".into(), prefix: None, print: true }),
            "the single-dash spellings still work"
        );
    }

    #[test]
    fn install_takes_its_files() {
        assert_eq!(
            parse_str("--install setup.exe fix.exe"),
            Ok(Command::Install(vec!["setup.exe".into(), "fix.exe".into()]))
        );
        assert_eq!(
            parse_str("--install a.exe --install b.exe"),
            Ok(Command::Install(vec!["a.exe".into(), "b.exe".into()]))
        );
    }

    /// `--launch` with no value fell through and opened the window; `--install` with no files did too.
    #[test]
    fn missing_values_are_errors() {
        for (args, err) in [
            ("--launch", "--launch needs a game"),
            ("--launch --print", "--launch needs a game"),
            ("--launch x --prefix", "--prefix needs a directory"),
            ("--launch x --prefix --print", "--prefix needs a directory"),
            ("--install", "--install needs an installer or a fix"),
            ("--install --snap a.png", "--install needs an installer or a fix"),
            ("--install a.exe --install", "--install needs an installer or a fix"),
            ("--snap", "--snap needs a value"),
        ] {
            assert_eq!(parse_str(args), Err(err.into()), "{args}");
        }
    }

    #[test]
    fn launch_options_need_launch() {
        assert_eq!(parse_str("--print"), Err("--print needs --launch".into()));
        assert_eq!(parse_str("--prefix /p"), Err("--prefix needs --launch".into()));
        assert_eq!(parse_str("--install a.exe --print"), Err("--print needs --launch".into()));
        assert_eq!(parse_str("--install a.exe --launch b"), Err("--launch and --install don't go together".into()));
    }

    /// A mistyped flag was silently ignored.
    #[test]
    fn unknown_arguments_are_errors() {
        assert_eq!(parse_str("--lauch game.exe"), Err("unknown option --lauch".into()));
        assert_eq!(parse_str("--snapp a.png"), Err("unknown option --snapp".into()));
        assert_eq!(parse_str("-x"), Err("unknown option -x".into()));
        assert_eq!(parse_str("game.exe"), Err("unexpected argument game.exe".into()));
        assert_eq!(parse_str("--launch a b"), Err("unexpected argument b".into()));
    }

    /// What scripts/preview.sh and CI pass: Rune's flags, with their values, open the window.
    #[test]
    fn runes_flags_pass_through() {
        for args in [
            "--snap out/library.png --size 1080x720",
            "--snap out/page.png --scene page --size 1280x760",
            "--snap out/hover.png --script /tmp/hover.json --at 0.8 --size 1080x720",
            "--tree t.json --at -1 --theme dark --scale 1 --fps 30 --frames 3 --out dir",
            "--trace t.jsonl --native --trace-all --list-scenes --first-pixels --chrome",
            "--strip s.png --crop 0,0,10,10 --every 2 --cols 4 --video v.mp4 --duration auto",
            "--keys --pointer --keys-theme light --desktop d.png --window main --list-windows",
            "--keys top-left --playground --variant 2",
            "--playground Story --snap a.png --crop card --whole-window --start mark:intro",
            "--html p.html --html-widths 360,640 --html-template t.html --scroll-bars always --hot",
        ] {
            assert_eq!(parse_str(args), Ok(Command::Window), "{args}");
        }
        // A value is Rune's even when it looks like a flag of ours.
        assert_eq!(parse_str("--scene --launch"), Ok(Command::Window));
        // --keys takes only a corner, so what follows is still checked.
        assert_eq!(parse_str("--keys game.exe"), Err("unexpected argument game.exe".into()));
    }
}
