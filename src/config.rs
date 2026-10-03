//! Multi-service config for `airshell-proxy`: one `[[service]]` per advertised
//! Bonjour record, each relaying to a local TCP service. Lives at
//! `$XDG_CONFIG_HOME/airshell/config.toml` (default `~/.config/airshell/config.toml`).
//!
//! ```toml
//! [[service]]
//! name = "ssh"
//! port = 22
//!
//! [[service]]
//! name = "db-mac"
//! service_type = "_awdldb._tcp"
//! port = 5432
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::SERVICE_TYPE;

/// Why a config file could not be turned into a usable set of services.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot parse config {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("config defines no [[service]] entries")]
    Empty,

    /// Two services share a Bonjour (name, service_type) pair and would collide.
    #[error("duplicate service: name {name:?} with service type {service_type:?} appears twice")]
    Duplicate { name: String, service_type: String },
}

/// A parsed config: an ordered list of services to advertise and relay.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub service: Vec<Service>,
}

/// One advertised service. `name` is required here (unlike the single-service
/// flags, which fall back to the computer name inside Network.framework): two
/// unnamed services could not be told apart on Bonjour or in the shared log.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Service {
    /// Bonjour service name to advertise.
    pub name: String,

    /// Bonjour service type to advertise.
    #[serde(default = "default_service_type")]
    pub service_type: String,

    /// Host of the local service to relay to.
    #[serde(default = "default_host")]
    pub host: String,

    /// Port of the local service to relay to.
    pub port: u16,
}

fn default_service_type() -> String {
    SERVICE_TYPE.to_owned()
}

fn default_host() -> String {
    "127.0.0.1".to_owned()
}

impl Config {
    /// Read and validate a config file.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        let config: Config = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Reject an empty config or colliding (name, service_type) pairs.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.service.is_empty() {
            return Err(ConfigError::Empty);
        }
        for (i, svc) in self.service.iter().enumerate() {
            if self.service[..i]
                .iter()
                .any(|prior| prior.name == svc.name && prior.service_type == svc.service_type)
            {
                return Err(ConfigError::Duplicate {
                    name: svc.name.clone(),
                    service_type: svc.service_type.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Default config path: `$XDG_CONFIG_HOME/airshell/config.toml`, else
/// `~/.config/airshell/config.toml`. `None` when neither env var is set.
pub fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("airshell").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_and_overrides() {
        let config: Config = toml::from_str(
            r#"
            [[service]]
            name = "ssh"
            port = 22

            [[service]]
            name = "db"
            service_type = "_awdldb._tcp"
            host = "127.0.0.1"
            port = 5432
            "#,
        )
        .unwrap();
        config.validate().unwrap();

        assert_eq!(config.service.len(), 2);
        assert_eq!(config.service[0].service_type, SERVICE_TYPE); // defaulted
        assert_eq!(config.service[0].host, "127.0.0.1"); // defaulted
        assert_eq!(config.service[1].service_type, "_awdldb._tcp");
        assert_eq!(config.service[1].port, 5432);
    }

    #[test]
    fn rejects_empty() {
        let config = Config { service: vec![] };
        assert!(matches!(config.validate(), Err(ConfigError::Empty)));
    }

    #[test]
    fn rejects_duplicate_name_and_service_type() {
        let config: Config = toml::from_str(
            r#"
            [[service]]
            name = "ssh"
            port = 22

            [[service]]
            name = "ssh"
            port = 2222
            "#,
        )
        .unwrap();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Duplicate { .. })
        ));
    }

    #[test]
    fn same_name_different_service_type_is_allowed() {
        let config: Config = toml::from_str(
            r#"
            [[service]]
            name = "box"
            service_type = "_awdlssh._tcp"
            port = 22

            [[service]]
            name = "box"
            service_type = "_awdldb._tcp"
            port = 5432
            "#,
        )
        .unwrap();
        config.validate().unwrap();
    }
}
