use std::env;
use std::path::PathBuf;

use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Port configuration: "auto" (random port) or a fixed port number (1-65535).
#[derive(Debug, Clone)]
pub enum PortConfig {
    Auto,
    Fixed(u16),
}

impl Default for PortConfig {
    fn default() -> Self {
        Self::Auto
    }
}

/// The application-level configuration merged from multiple sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub port: PortConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: PortConfig::Auto,
        }
    }
}

impl Config {
    /// Load configuration by merging sources with ascending priority:
    ///   1. default values
    ///   2. `$HOME/.config/lantype/config.json` (global)
    ///   3. `./config.json` (local, project/cwd)
    ///
    /// Merging is shallow at the top level — keys in higher-priority sources
    /// replace those in lower-priority sources entirely.
    pub fn load() -> Self {
        let mut merged = serde_json::json!({});

        // Global config: $HOME/.config/lantype/config.json
        if let Some(global_path) = global_config_path() {
            if global_path.exists() {
                match std::fs::read_to_string(&global_path) {
                    Ok(content) => match serde_json::from_str::<Value>(&content) {
                        Ok(val) => merge(&mut merged, val),
                        Err(e) => warn!("Failed to parse global config at {}: {e}", global_path.display()),
                    },
                    Err(e) => warn!("Failed to read global config at {}: {e}", global_path.display()),
                }
            }
        }

        // Local config: ./config.json (current working directory)
        let local_path = PathBuf::from("config.json");
        if local_path.exists() {
            match std::fs::read_to_string(&local_path) {
                Ok(content) => match serde_json::from_str::<Value>(&content) {
                    Ok(val) => merge(&mut merged, val),
                    Err(e) => warn!("Failed to parse local config at {}: {e}", local_path.display()),
                },
                Err(e) => warn!("Failed to read local config at {}: {e}", local_path.display()),
            }
        }

        serde_json::from_value(merged).unwrap_or_else(|e| {
            warn!("Failed to deserialize merged config, using defaults: {e}");
            Config::default()
        })
    }
}

// ---- Serialisation helpers ----

impl<'de> Deserialize<'de> for PortConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(s) if s == "auto" => Ok(PortConfig::Auto),
            Value::Number(n) => {
                let port = n.as_u64().ok_or_else(|| {
                    serde::de::Error::custom("expected a positive integer for port")
                })?;
                if port == 0 || port > 65535 {
                    return Err(serde::de::Error::custom(
                        "port must be in range 1-65535",
                    ));
                }
                Ok(PortConfig::Fixed(port as u16))
            }
            _ => Err(serde::de::Error::custom(
                "expected \"auto\" or a port number (1-65535)",
            )),
        }
    }
}

impl Serialize for PortConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            PortConfig::Auto => serializer.serialize_str("auto"),
            PortConfig::Fixed(port) => serializer.serialize_u16(*port),
        }
    }
}

// ---- Helpers ----

/// Shallow top-level merge: keys from `override_val` replace those in `base`.
fn merge(base: &mut Value, override_val: Value) {
    if let (Value::Object(base_map), Value::Object(override_map)) = (base, override_val) {
        for (k, v) in override_map {
            base_map.insert(k, v);
        }
    }
}

/// Returns the path to the global config file:
/// `$HOME/.config/lantype/config.json`
fn global_config_path() -> Option<PathBuf> {
    env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("lantype")
            .join("config.json")
    })
}