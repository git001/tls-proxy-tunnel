use std::fmt;
use std::io::Error as IOError;

#[derive(Debug)]
pub enum ConfigError {
    IO(IOError),
    Yaml(serde_yaml_ng::Error),
    Custom(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::IO(e) => write!(f, "IO error: {}", e),
            ConfigError::Yaml(e) => write!(f, "YAML parse error: {}", e),
            ConfigError::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::IO(e) => Some(e),
            ConfigError::Yaml(e) => Some(e),
            ConfigError::Custom(_) => None,
        }
    }
}

impl From<IOError> for ConfigError {
    fn from(err: IOError) -> ConfigError {
        ConfigError::IO(err)
    }
}

impl From<serde_yaml_ng::Error> for ConfigError {
    fn from(err: serde_yaml_ng::Error) -> ConfigError {
        ConfigError::Yaml(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn make_yaml_error() -> serde_yaml_ng::Error {
        serde_yaml_ng::from_str::<std::collections::HashMap<String, u32>>("key: not-a-number")
            .unwrap_err()
    }

    #[test]
    fn test_io_source_is_some() {
        let err = ConfigError::IO(std::io::Error::new(std::io::ErrorKind::NotFound, "x"));
        assert!(err.source().is_some());
    }

    #[test]
    fn test_yaml_source_is_some() {
        let err = ConfigError::Yaml(make_yaml_error());
        assert!(err.source().is_some());
    }

    #[test]
    fn test_custom_source_is_none() {
        let err = ConfigError::Custom("oops".to_string());
        assert!(err.source().is_none());
    }

    #[test]
    fn test_io_display() {
        let err = ConfigError::IO(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(err.to_string().contains("IO error"));
    }

    #[test]
    fn test_yaml_display() {
        let err = ConfigError::Yaml(make_yaml_error());
        assert!(err.to_string().contains("YAML"));
    }

    #[test]
    fn test_custom_display() {
        let err = ConfigError::Custom("something went wrong".to_string());
        assert_eq!(err.to_string(), "something went wrong");
    }
}
