use crate::upstreams::ProxyToUpstream;
use crate::upstreams::Upstream;
use log::{debug, info, warn};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Error as IOError, Read};
use url::Url;

#[derive(Debug, Clone)]
pub struct ConfigV1 {
    pub base: ParsedConfigV1,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ParsedConfigV1 {
    pub version: i32,
    pub log: Option<String>,
    pub servers: HashMap<String, ServerConfig>,
    pub upstream: HashMap<String, Upstream>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct BaseConfig {
    pub version: i32,
    pub log: Option<String>,
    pub servers: HashMap<String, ServerConfig>,
    pub upstream: HashMap<String, String>,
    pub via: ViaUpstream,
}
#[derive(Debug, Default, Deserialize, Clone)]
pub struct ViaUpstream {
    /*
     * Hold the Headers which sould send to the Upstream Proxy
     */
    pub headers: HashMap<String, String>,

    /*
     * Hold the Upstream target Proxy
     */
    pub target: String,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen: Vec<String>,
    pub protocol: Option<String>,
    pub tls: Option<bool>,
    pub sni: Option<HashMap<String, String>>,
    pub default: Option<String>,
    pub via: ViaUpstream,
}
impl TryInto<ProxyToUpstream> for &str {
    type Error = ConfigError;

    fn try_into(self) -> Result<ProxyToUpstream, Self::Error> {
        let upstream_url = match Url::parse(self) {
            Ok(url) => url,
            Err(_) => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream url {}",
                    self
                )))
            }
        };

        let upstream_host = match upstream_url.host_str() {
            Some(host) => host,
            None => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream url {}",
                    self
                )))
            }
        };

        let upstream_port = match upstream_url.port_or_known_default() {
            Some(port) => port,
            None => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream url {}",
                    self
                )))
            }
        };

        match upstream_url.scheme() {
            "tcp" | "tcp4" | "tcp6" => {}
            _ => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream scheme {}",
                    self
                )))
            }
        }

        Ok(ProxyToUpstream::new(
            format!("{}:{}", upstream_host, upstream_port),
            upstream_url.scheme().to_string(),
        ))
    }
}

#[derive(Debug)]
pub enum ConfigError {
    IO(IOError),
    Yaml(serde_yml::Error),
    Custom(String),
}

impl ConfigV1 {
    pub fn new(path: &str) -> Result<ConfigV1, ConfigError> {
        let base = load_config(path)?;

        Ok(ConfigV1 { base })
    }
}

fn load_config(path: &str) -> Result<ParsedConfigV1, ConfigError> {
    let mut contents = String::new();
    let mut file = File::open(path)?;
    file.read_to_string(&mut contents)?;

    let base: BaseConfig = serde_yml::from_str(&contents).unwrap();

    if base.version != 1 {
        return Err(ConfigError::Custom(
            "Unsupported config version".to_string(),
        ));
    }

    let log_level = base.log.clone().unwrap_or_else(|| "info".to_string());
    if !log_level.eq("disable") {
        std::env::set_var("FOURTH_LOG", log_level.clone());
        pretty_env_logger::init_custom_env("FOURTH_LOG");
    }

    info!("Using config file: {}", &path);
    debug!("Set log level to {}", log_level);
    debug!("Config version {}", base.version);

    let mut parsed_upstream: HashMap<String, Upstream> = HashMap::new();

    parsed_upstream.insert("ban".to_string(), Upstream::Ban);
    parsed_upstream.insert("echo".to_string(), Upstream::Echo);
    parsed_upstream.insert("health".to_string(), Upstream::Health);

    for (name, upstream) in base.upstream.iter() {
        let ups = upstream.as_str().try_into()?;
        parsed_upstream.insert(name.to_string(), Upstream::Proxy(ups));
    }
    let via: ViaUpstream = base.via.clone();
    debug!("via {:?}", via);

    let parsed = ParsedConfigV1 {
        version: base.version,
        log: base.log,
        servers: base.servers,
        upstream: parsed_upstream,
    };

    verify_config(parsed)
}

fn verify_config(config: ParsedConfigV1) -> Result<ParsedConfigV1, ConfigError> {
    let mut used_upstreams: HashSet<String> = HashSet::new();
    let mut upstream_names: HashSet<String> = HashSet::new();
    let mut listen_addresses: HashSet<String> = HashSet::new();

    debug!("Version: {:?}", config.version);
    debug!("Log: {:?}", config.log);

    // Check for duplicate upstream names
    for (name, _) in config.upstream.iter() {
        if upstream_names.contains(name) {
            return Err(ConfigError::Custom(format!(
                "Duplicate upstream name {}",
                name
            )));
        }

        upstream_names.insert(name.to_string());
    }

    for (_, server) in config.servers.clone() {
        // check for duplicate listen addresses
        for listen in server.listen {
            if listen_addresses.contains(&listen) {
                return Err(ConfigError::Custom(format!(
                    "Duplicate listen address {}",
                    listen
                )));
            }

            listen_addresses.insert(listen.to_string());
        }

        if server.tls.unwrap_or_default() && server.sni.is_some() {
            for (_, val) in server.sni.unwrap() {
                used_upstreams.insert(val.to_string());
            }
        }

        if server.default.is_some() {
            used_upstreams.insert(server.default.unwrap().to_string());
        }

        for key in &used_upstreams {
            if !config.upstream.contains_key(key) {
                return Err(ConfigError::Custom(format!("Upstream {} not found", key)));
            }
        }
    }

    for key in &upstream_names {
        if !used_upstreams.contains(key) && !key.eq("echo") && !key.eq("ban") {
            warn!("Upstream {} not used", key);
        }
    }

    Ok(config)
}

impl From<IOError> for ConfigError {
    fn from(err: IOError) -> ConfigError {
        ConfigError::IO(err)
    }
}

impl From<serde_yml::Error> for ConfigError {
    fn from(err: serde_yml::Error) -> ConfigError {
        ConfigError::Yaml(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let config = ConfigV1::new("tests/config.yaml").unwrap();
        assert_eq!(config.base.version, 1);
        assert_eq!(config.base.log.unwrap(), "disable");
        assert_eq!(config.base.servers.len(), 3);
        assert_eq!(config.base.upstream.len(), 3 + 2); // Add ban and echo upstreams
    }
}
