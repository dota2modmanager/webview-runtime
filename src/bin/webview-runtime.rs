//! A command line over the crate, for testing an engine by hand and for CI.
//!
//!   webview-runtime status  [--base DIR] [--manifest FILE]
//!   webview-runtime install [--base DIR] [--manifest FILE] [--from ARCHIVE]
//!   webview-runtime run     [--base DIR] [--manifest FILE] [--software] -- PROGRAM ARGS...
//!
//! `run` starts PROGRAM on our own engine: how the app would start its window with it.

use std::path::PathBuf;
use std::process::ExitCode;

use webview_runtime::{env_for, store, system, Manifest, MANIFEST};

struct Args {
    command: String,
    base: PathBuf,
    manifest: Option<PathBuf>,
    from: Option<PathBuf>,
    software: bool,
    rest: Vec<String>,
}

fn default_base() -> PathBuf {
    let data = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    data.unwrap_or_else(|| ".".into()).join("dota2-mod-manager").join("webview-runtime")
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let command = it.next().ok_or("a command: status, install or run")?;
    let mut a = Args { command, base: default_base(), manifest: None, from: None, software: false, rest: Vec::new() };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--base" => a.base = it.next().ok_or("--base needs a folder")?.into(),
            "--manifest" => a.manifest = Some(it.next().ok_or("--manifest needs a file")?.into()),
            "--from" => a.from = Some(it.next().ok_or("--from needs an archive")?.into()),
            "--software" => a.software = true,
            "--" => {
                a.rest = it.collect();
                break;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn run() -> Result<ExitCode, String> {
    let a = parse()?;
    let text = match &a.manifest {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?,
        None => MANIFEST.to_string(),
    };
    let manifest = Manifest::parse(&text).map_err(|e| e.to_string())?;
    let runtime = manifest.for_this_platform();
    match a.command.as_str() {
        "status" => {
            println!("system engine present: {}", system::engine_present());
            match runtime {
                None => println!("own engine: none known for this platform"),
                Some(r) => {
                    println!("own engine: {} {} ({} bytes)", r.platform.name(), r.version, r.size);
                    match store::find(&a.base, r) {
                        Some(i) => println!("installed: {}", i.dir.display()),
                        None => println!("installed: no ({})", a.base.display()),
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        "install" => {
            let r = runtime.ok_or("no engine is known for this platform")?;
            let got = match &a.from {
                Some(archive) => store::install_from(&a.base, r, archive),
                None => {
                    let mut last = 0;
                    store::install(&a.base, r, &mut |done, size| {
                        let pct = done * 100 / size.max(1);
                        if pct >= last + 10 {
                            last = pct;
                            eprintln!("{pct}%");
                        }
                    })
                }
            }
            .map_err(|e| e.to_string())?;
            println!("{}", got.dir.display());
            Ok(ExitCode::SUCCESS)
        }
        "run" => {
            let r = runtime.ok_or("no engine is known for this platform")?;
            let installed = store::find(&a.base, r).ok_or("not installed: run install first")?;
            let (program, args) = a.rest.split_first().ok_or("run needs -- PROGRAM")?;
            let launch = env_for(&installed, a.software);
            let mut cmd = std::process::Command::new(program);
            cmd.args(args).envs(launch.vars.iter().map(|(k, v)| (k, v)));
            if let Some(cwd) = &launch.cwd {
                if let Ok(owd) = std::env::current_dir() {
                    cmd.env("D2MM_OWD", owd);
                }
                cmd.current_dir(cwd);
            }
            let status = cmd.status().map_err(|e| format!("{program}: {e}"))?;
            Ok(ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8))
        }
        other => Err(format!("unknown command {other}")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("webview-runtime: {e}");
            ExitCode::from(2)
        }
    }
}
