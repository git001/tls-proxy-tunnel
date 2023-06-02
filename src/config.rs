use log::{debug, warn};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Error as IOError, Read};
use std::net::SocketAddr;
use tokio::sync::Mutex;
use url::Url;
use tokio::time::Instant;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct Config {
    pub base: ParsedConfig,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ParsedConfig {
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
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen: Vec<String>,
    pub protocol: Option<String>,
    pub tls: Option<bool>,
    pub sni: Option<HashMap<String, String>>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Upstream {
    Ban,
    Echo,
    Custom(CustomUpstream),
}

#[derive(Debug)]
struct Addr(Mutex<Vec<SocketAddr>>);

impl Default for Addr {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl Clone for Addr {
    fn clone(&self) -> Self {
        tokio::task::block_in_place(|| Self(Mutex::new(self.0.blocking_lock().clone())))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomUpstream {
    pub name: String,
    pub addr: String,
    pub protocol: String,
    #[serde(skip_deserializing)]
    addresses: Addr,
}

impl CustomUpstream {
    pub async fn resolve_addresses(&self) -> std::io::Result<()> {
        {
            let addr = self.addresses.0.lock().await;
            if addr.len() > 0 {
                debug!("Already have addresses: {:?}", &addr);
                return Ok(());
            }
        }

        debug!("Resolving addresses for {}", &self.addr);
        let addresses = tokio::net::lookup_host(self.addr.clone()).await?;

        let mut addr: Vec<SocketAddr> = match self.protocol.as_ref() {
            "tcp4" => addresses.into_iter().filter(|a| a.is_ipv4()).collect(),
            "tcp6" => addresses.into_iter().filter(|a| a.is_ipv6()).collect(),
            _ => addresses.collect(),
        };

        debug!("Got addresses for {}: {:?}", &self.addr, &addr);
        debug!("Resolved at {}", OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).expect("Format"));

        {
            let mut self_addr = self.addresses.0.lock().await;
            self_addr.clear();
            self_addr.append(&mut addr);
        }
        Ok(())
    }

    pub async fn get_addresses(&self) -> Vec<SocketAddr> {
        let a = self.addresses.0.lock().await;
        a.clone()
    }
}

impl Default for CustomUpstream {
    fn default() -> Self {
        Self {
            name: Default::default(),
            addr: Default::default(),
            protocol: Default::default(),
            addresses: Default::default(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    IO(IOError),
    Yaml(serde_yaml::Error),
    Custom(String),
}

impl Config {
    pub fn new(path: &str) -> Result<Config, ConfigError> {
        let base = (load_config(path))?;

        Ok(Config { base })
    }
}

fn load_config(path: &str) -> Result<ParsedConfig, ConfigError> {
    let mut contents = String::new();
    let mut file = (File::open(path))?;
    (file.read_to_string(&mut contents))?;

    let base: BaseConfig = serde_yaml::from_str(&contents)?;

    if base.version != 1 {
        return Err(ConfigError::Custom(
            "Unsupported config version".to_string(),
        ));
    }

    let log_level = base.log.clone().unwrap_or_else(|| "info".to_string());
    if !log_level.eq("disable") {
        std::env::set_var("FOURTH_LOG", log_level.clone());
        pretty_env_logger::init_custom_env("FOURTH_LOG");
        debug!("Set log level to {}", log_level);
    }

    debug!("Config version {}", base.version);

    let mut parsed_upstream: HashMap<String, Upstream> = HashMap::new();

    for (name, upstream) in base.upstream.iter() {
        let upstream_url = match Url::parse(upstream) {
            Ok(url) => url,
            Err(_) => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream url {}",
                    upstream
                )))
            }
        };

        let upstream_host = match upstream_url.host_str() {
            Some(host) => host,
            None => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream url {}",
                    upstream
                )))
            }
        };

        let upsteam_port = match upstream_url.port_or_known_default() {
            Some(port) => port,
            None => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream url {}",
                    upstream
                )))
            }
        };

        match upstream_url.scheme() {
            "tcp" | "tcp4" | "tcp6" => {}
            _ => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream scheme {}",
                    upstream
                )))
            }
        }

        parsed_upstream.insert(
            name.to_string(),
            Upstream::Custom(CustomUpstream {
                name: name.to_string(),
                addr: format!("{}:{}", upstream_host, upsteam_port),
                protocol: upstream_url.scheme().to_string(),
                ..Default::default()
            }),
        );
    }

    parsed_upstream.insert("ban".to_string(), Upstream::Ban);

    parsed_upstream.insert("echo".to_string(), Upstream::Echo);

    let parsed = ParsedConfig {
        version: base.version,
        log: base.log,
        servers: base.servers,
        upstream: parsed_upstream,
    };

    verify_config(parsed)
}

fn verify_config(config: ParsedConfig) -> Result<ParsedConfig, ConfigError> {
    let mut used_upstreams: HashSet<String> = HashSet::new();
    let mut upstream_names: HashSet<String> = HashSet::new();
    let mut listen_addresses: HashSet<String> = HashSet::new();

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

impl From<serde_yaml::Error> for ConfigError {
    fn from(err: serde_yaml::Error) -> ConfigError {
        ConfigError::Yaml(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let config = Config::new("tests/config.yaml").unwrap();
        assert_eq!(config.base.version, 1);
        assert_eq!(config.base.log.unwrap(), "disable");
        assert_eq!(config.base.servers.len(), 5);
        assert_eq!(config.base.upstream.len(), 3 + 2); // Add ban and echo upstreams
    }
}
