use log::{debug, info, warn};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use url::Url;

use crate::upstreams::{ProxyToUpstream, Upstream};

use super::error::ConfigError;
use super::types::{BaseConfig, Config, ParsedConfig};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

impl Config {
    pub fn new(path: &str) -> Result<Config, ConfigError> {
        let base = load_config(path)?;
        Ok(Config { base })
    }
}

// ---------------------------------------------------------------------------
// URL → ProxyToUpstream
// ---------------------------------------------------------------------------

impl TryFrom<&str> for ProxyToUpstream {
    type Error = ConfigError;

    fn try_from(value: &str) -> Result<ProxyToUpstream, Self::Error> {
        let upstream_url = Url::parse(value)
            .map_err(|_| ConfigError::Custom(format!("Invalid upstream url {}", value)))?;

        let upstream_host = upstream_url
            .host_str()
            .ok_or_else(|| ConfigError::Custom(format!("Invalid upstream url {}", value)))?;

        let upstream_port = upstream_url
            .port_or_known_default()
            .ok_or_else(|| ConfigError::Custom(format!("Invalid upstream url {}", value)))?;

        match upstream_url.scheme() {
            "tcp" | "tcp4" | "tcp6" => {}
            _ => {
                return Err(ConfigError::Custom(format!(
                    "Invalid upstream scheme {}",
                    value
                )));
            }
        }

        Ok(ProxyToUpstream::new(
            format!("{}:{}", upstream_host, upstream_port),
            upstream_url.scheme().to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Load + parse
// ---------------------------------------------------------------------------

fn load_config(path: &str) -> Result<ParsedConfig, ConfigError> {
    let mut contents = String::new();
    File::open(path)?.read_to_string(&mut contents)?;

    let base: BaseConfig = serde_yaml_ng::from_str(&contents)?;

    if !matches!(base.version, 1 | 2) {
        return Err(ConfigError::Custom(format!(
            "Unsupported config version {}",
            base.version
        )));
    }

    let log_level = base.log.clone().unwrap_or_else(|| "info".to_string());
    let log_format = base.log_format.clone().unwrap_or_else(|| "txt".to_string());

    match log_format.as_str() {
        "txt" | "text" | "json" => {}
        other => {
            return Err(ConfigError::Custom(format!(
                "Invalid log-format '{}': must be 'txt', 'text' or 'json'",
                other
            )));
        }
    }

    if !log_level.eq("disable") {
        let mut builder = env_logger::builder();
        // RUST_LOG env var takes precedence over config file log level
        if std::env::var("RUST_LOG").is_err() {
            builder.parse_filters(&log_level);
        }
        if log_format == "json" {
            builder.format(|buf, record| {
                use std::io::Write;
                let ts = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                let msg = serde_json::Value::String(record.args().to_string());
                writeln!(
                    buf,
                    r#"{{"ts":"{ts}","level":"{level}","target":"{target}","msg":{msg}}}"#,
                    ts = ts,
                    level = record.level(),
                    target = record.target(),
                    msg = msg,
                )
            });
        } else {
            builder.default_format();
        }
        let _ = builder.try_init();
        info!("tls-proxy-tunnel v{}", env!("CARGO_PKG_VERSION"));
    }

    info!("Using config file: {}", path);
    debug!("Set log level to {}", log_level);
    debug!("Config version {}", base.version);

    let mut upstream: HashMap<String, Upstream> = HashMap::from([
        ("ban".to_string(), Upstream::Ban),
        ("echo".to_string(), Upstream::Echo),
        // Metrics are injected later by From<ParsedConfig> for Server after all
        // proxies (and their semaphores) are built.
        (
            "health".to_string(),
            Upstream::Health(std::sync::Arc::new(vec![])),
        ),
    ]);
    for (name, url) in &base.upstream {
        upstream.insert(
            name.clone(),
            Upstream::Proxy(ProxyToUpstream::try_from(url.as_str())?),
        );
    }

    let parsed = ParsedConfig {
        version: base.version,
        log: base.log,
        servers: base.servers,
        upstream,
    };

    verify_config(parsed)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn verify_config(config: ParsedConfig) -> Result<ParsedConfig, ConfigError> {
    let upstream_names: HashSet<String> = config.upstream.keys().cloned().collect();
    let mut used_upstreams: HashSet<String> = HashSet::new();
    let mut listen_addresses: HashSet<String> = HashSet::new();

    debug!("Version: {:?}", config.version);
    debug!("Log: {:?}", config.log);

    for server in config.servers.values() {
        for listen in &server.listen {
            if listen_addresses.contains(listen.as_str()) {
                return Err(ConfigError::Custom(format!(
                    "Duplicate listen address {}",
                    listen
                )));
            }
            listen_addresses.insert(listen.clone());
        }

        if server.tls.unwrap_or_default()
            && let Some(sni_map) = &server.sni
        {
            for target in sni_map.values() {
                used_upstreams.insert(target.upstream_name().to_string());
            }
        }

        if let Some(default) = &server.default {
            used_upstreams.insert(default.clone());
        }

        for key in &used_upstreams {
            if !config.upstream.contains_key(key) {
                return Err(ConfigError::Custom(format!("Upstream {} not found", key)));
            }
        }
    }

    for key in &upstream_names {
        if !used_upstreams.contains(key) && !matches!(key.as_str(), "echo" | "ban" | "health") {
            warn!("Upstream {} not used", key);
        }
    }

    Ok(config)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
