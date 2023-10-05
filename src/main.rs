mod config;
mod plugins;
mod servers;
mod upstreams;

use crate::config::ConfigV1;
use crate::servers::Server;

use log::{debug, error};
use std::env;
use std::path::Path;

fn main() {
    let config_path = find_config();

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

fn find_config() -> String {
    let config_path =
        env::var("FOURTH_CONFIG").unwrap_or_else(|_| "/etc/fourth/config.yaml".to_string());

    if Path::new(&config_path).exists() {
        return config_path;
    }

    if Path::new("config.yaml").exists() {
        return String::from("config.yaml");
    }

    String::from("")
}
