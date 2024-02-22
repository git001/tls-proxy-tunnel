mod config;
mod plugins;
mod servers;
mod upstreams;

use crate::config::ConfigV1;
use crate::servers::Server;

use log::{debug, error};
use std::path::PathBuf;

fn main() {
    let config_path = match find_config() {
        Ok(p) => p,
        Err(paths) => {
            println!("Could not find config file. Tried paths:");
            for p in paths {
                println!("- {}", p);
            }
            std::process::exit(1);
        }
    };

    let config = match ConfigV1::new(&config_path) {
        Ok(config) => config,
        Err(e) => {
            println!("Could not load config: {:?}", e);
            std::process::exit(1);
        }
    };
    debug!("{:?}", config);

    let mut server = Server::new_from_v1_config(config.base);
    debug!("{:?}", server);

    let _ = server.run();
    error!("Server ended with errors");
}

fn find_config() -> Result<String, Vec<String>> {
    let possible_paths = ["/etc/l4p", ""];
    let possible_names = ["l4p.yaml", "config.yaml"];

    let mut tried_paths = Vec::<String>::new();

    for path in possible_paths
        .iter()
        .flat_map(|&path| {
            possible_names
                .iter()
                .map(move |&file| PathBuf::new().join(path).join(file))
        })
        .collect::<Vec<PathBuf>>()
    {
        let path_str = path.to_string_lossy().to_string();
        if path.exists() {
            return Ok(path_str);
        }

        tried_paths.push(path_str);
    }

    Err(tried_paths)
}
