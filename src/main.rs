mod config;
mod servers;
mod upstreams;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use crate::config::Config;
use crate::servers::Server;

use log::{debug, error, info};
use std::path::PathBuf;
use std::process::ExitCode;

fn print_help() {
    println!(concat!(
        "tls-proxy-tunnel (tpt) v",
        env!("CARGO_PKG_VERSION"),
        "\n\
             \n\
             USAGE:\n\
             \ttpt [OPTIONS]\n\
             \n\
             OPTIONS:\n\
             \t-c, --config <path>    Path to config file\n\
             \t-h, --help             Show this help\n\
             \n\
             CONFIG SEARCH ORDER (when --config is not given):\n\
             \t1. $TPT_CONFIG environment variable\n\
             \t2. /etc/tpt/tpt.yaml\n\
             \t3. /etc/tpt/config.yaml\n\
             \t4. ./tpt.yaml\n\
             \t5. ./config.yaml\n"
    ));
}

#[derive(Debug)]
enum Cli {
    Help,
    Run { config_path: Option<String> },
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Cli::Help);
    }
    match args.first().map(String::as_str) {
        Some("--config" | "-c") => match args.get(1) {
            Some(path) => Ok(Cli::Run {
                config_path: Some(path.clone()),
            }),
            None => Err("--config requires a path argument".to_string()),
        },
        Some(other) => Err(format!("Unknown argument: {other}")),
        None => Ok(Cli::Run { config_path: None }),
    }
}

fn find_config() -> Result<String, Vec<String>> {
    find_config_in(std::env::var("TPT_CONFIG").ok(), &["/etc/tpt", ""])
}

/// Search order: `env_path` (from $TPT_CONFIG), then `tpt.yaml` and
/// `config.yaml` in each of `dirs` ("" = current directory). Separate from
/// find_config() so tests need not change the process-wide env or cwd.
fn find_config_in(env_path: Option<String>, dirs: &[&str]) -> Result<String, Vec<String>> {
    let paths = env_path.map(PathBuf::from).into_iter().chain(
        dirs.iter()
            .flat_map(|&dir| ["tpt.yaml", "config.yaml"].map(|f| PathBuf::from(dir).join(f))),
    );

    let mut tried = Vec::new();
    for path in paths {
        let s = path.to_string_lossy().into_owned();
        if path.exists() {
            return Ok(s);
        }
        tried.push(s);
    }
    Err(tried)
}

fn run(args: &[String]) -> Result<(), u8> {
    let cli = match parse_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            eprintln!("Run with --help for usage.");
            return Err(1);
        }
    };

    let config_path = match cli {
        Cli::Help => {
            print_help();
            return Ok(());
        }
        Cli::Run {
            config_path: Some(p),
        } => p,
        // parse_args only yields `config_path: None` without arguments:
        // no config found → show the help.
        Cli::Run { config_path: None } => match find_config() {
            Ok(p) => p,
            Err(_) => {
                print_help();
                return Ok(());
            }
        },
    };

    let config = match Config::new(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Could not load config: {}", e);
            return Err(1);
        }
    };

    debug!("{:?}", config);

    let mut server = Server::from(config.base);
    info!("{:?}", server);

    if let Err(e) = server.run() {
        error!("Server ended with error: {:?}", e);
        return Err(1);
    }

    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
