pub mod commands;
use std::{path::PathBuf, process};

use clap::Parser;

use crate::channel_state::ChannelState;

fn datadir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = {
        let mut dir = dirs::home_dir().unwrap();
        dir.push(".lfc");
        dir
    };

    #[cfg(not(target_os = "linux"))]
    let dir = {
        let mut dir = dirs::config_dir().unwrap();
        dir.push("Lfc");
        dir
    };

    maybe_create_dir(&dir);

    dir
}

fn maybe_create_dir(dir: &PathBuf) {
    if !dir.exists() {
        #[cfg(unix)]
        {
            use std::fs::DirBuilder;
            use std::os::unix::fs::DirBuilderExt;

            let mut builder = DirBuilder::new();
            builder.mode(0o700).recursive(true).create(dir).unwrap();
        }

        #[cfg(not(unix))]
        std::fs::create_dir_all(dir).unwrap();
    }
}

pub fn parse() -> Args {
    let mut args = Args::parse();
    let mut conf = if let Some(dir) = args.datadir.as_ref() {
        maybe_create_dir(dir);
        dir.clone()
    } else {
        datadir()
    };
    let mut path = args.wallet.clone();
    match path.is_empty() {
        false => {
            if path.contains("/") {
                eprintln!("wallet must be a name, not a path!");
                process::exit(1);
            }
            if !path.ends_with(".conf") {
                path = format!("{}.conf", path);
            }
            conf.push(path);
        }
        true => {
            conf.push("lfc.conf");
        }
    }

    if !matches!(args.command, commands::Command::Conf { .. }) {
        if !conf.exists() {
            eprintln!("wallet {} does not exists! ({:?})", args.wallet, conf);
            process::exit(1);
        }
        if !conf.is_file() {
            eprintln!("wallet {} is not a file! ({:?})", args.wallet, conf);
            process::exit(1);
        }
    } else if conf.exists() {
        eprintln!("wallet {} already exists!", args.wallet);
        process::exit(1);
    }
    args.path = conf.clone();
    if let Some(state) = args.state.as_mut() {
        state.rounds.init();
        state.path = conf;
    }
    args
}

#[derive(Parser, Debug)]
pub struct Args {
    #[command(subcommand)]
    pub command: commands::Command,
    /// Wallet name
    #[arg(global = true, default_value = "lfc")]
    pub wallet: String,
    /// option to output raw json to stdout
    #[arg(short, long, default_value_t = false)]
    pub raw: bool,
    /// Path supplied by user
    #[arg(short, long)]
    pub datadir: Option<PathBuf>,

    /// Validated file path
    #[arg(skip)]
    pub path: PathBuf,
    /// Parsed state
    #[arg(skip)]
    pub state: Option<ChannelState>,
}
