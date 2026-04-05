use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const DEFAULT_CONFIG_PATH: &str = "/etc/tuxedo-daemon/config.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub fan: FanConfig,
    #[serde(default)]
    pub keyboard: KeyboardConfig,
    #[serde(default)]
    pub profile: ProfileConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanConfig {
    #[serde(default = "default_fan_mode")]
    pub mode: FanMode,
    #[serde(default = "default_min_speed_percent")]
    pub min_speed_percent: u8,
    #[serde(default = "default_fan_curve")]
    pub curve: Vec<FanCurvePoint>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FanMode {
    Auto,
    Manual,
    CustomCurve,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FanCurvePoint {
    pub temp: u8,
    pub speed: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardConfig {
    #[serde(default = "default_kbd_mode")]
    pub mode: String,
    #[serde(default = "default_brightness")]
    pub brightness: u8,
    #[serde(default = "default_color")]
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    #[serde(default = "default_profile")]
    pub default: String,
}

fn default_fan_mode() -> FanMode {
    FanMode::Auto
}
fn default_min_speed_percent() -> u8 {
    25
}
fn default_fan_curve() -> Vec<FanCurvePoint> {
    vec![
        FanCurvePoint { temp: 40, speed: 0 },
        FanCurvePoint { temp: 60, speed: 30 },
        FanCurvePoint { temp: 80, speed: 80 },
        FanCurvePoint { temp: 90, speed: 100 },
    ]
}
fn default_kbd_mode() -> String {
    "static".to_string()
}
fn default_brightness() -> u8 {
    50
}
fn default_color() -> String {
    "#ffffff".to_string()
}
fn default_profile() -> String {
    "balanced".to_string()
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            mode: default_fan_mode(),
            min_speed_percent: default_min_speed_percent(),
            curve: default_fan_curve(),
        }
    }
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            mode: default_kbd_mode(),
            brightness: default_brightness(),
            color: default_color(),
        }
    }
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            default: default_profile(),
        }
    }
}

impl Config {
    /// Load config from the default path, or return defaults if not found.
    pub fn load_or_default() -> Self {
        match Self::load_from(Path::new(DEFAULT_CONFIG_PATH)) {
            Ok(config) => config,
            Err(ConfigError::Io(_, _)) => {
                info!("no config file at {DEFAULT_CONFIG_PATH}, using defaults");
                Config::default()
            }
            Err(e) => {
                warn!("config error: {e} — falling back to defaults");
                Config::default()
            }
        }
    }

    /// Load config from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e, path.to_path_buf()))?;
        let config: Config = toml::from_str(&content).map_err(ConfigError::Parse)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate config values.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.fan.min_speed_percent > 100 {
            return Err(ConfigError::Invalid(
                "fan.min_speed_percent must be 0-100".to_string(),
            ));
        }

        for (i, point) in self.fan.curve.iter().enumerate() {
            if point.speed > 100 {
                return Err(ConfigError::Invalid(format!(
                    "fan.curve[{i}].speed must be 0-100"
                )));
            }
        }

        // Curve must be sorted by temperature
        for window in self.fan.curve.windows(2) {
            if window[1].temp <= window[0].temp {
                return Err(ConfigError::Invalid(
                    "fan.curve temperatures must be strictly increasing".to_string(),
                ));
            }
        }

        if self.keyboard.brightness > 100 {
            warn!("keyboard.brightness > 100 — will be clamped per device limits");
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error, PathBuf),
    Parse(toml::de::Error),
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e, path) => write!(f, "failed to read {}: {e}", path.display()),
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
            ConfigError::Invalid(msg) => write!(f, "config validation error: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.fan.mode, FanMode::Auto);
        assert_eq!(config.fan.min_speed_percent, 25);
        assert_eq!(config.fan.curve.len(), 4);
        assert_eq!(config.keyboard.brightness, 50);
        assert_eq!(config.profile.default, "balanced");
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = "";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.fan.mode, FanMode::Auto);
    }

    #[test]
    fn test_parse_full_toml() {
        let toml_str = r##"
[fan]
mode = "custom-curve"
min_speed_percent = 30

[[fan.curve]]
temp = 40
speed = 0

[[fan.curve]]
temp = 70
speed = 50

[[fan.curve]]
temp = 90
speed = 100

[keyboard]
mode = "breath"
brightness = 80
color = "#ff0000"

[profile]
default = "performance"
"##;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.fan.mode, FanMode::CustomCurve);
        assert_eq!(config.fan.min_speed_percent, 30);
        assert_eq!(config.fan.curve.len(), 3);
        assert_eq!(config.keyboard.mode, "breath");
        assert_eq!(config.profile.default, "performance");
    }

    #[test]
    fn test_validate_bad_curve_order() {
        let config = Config {
            fan: FanConfig {
                curve: vec![
                    FanCurvePoint { temp: 60, speed: 30 },
                    FanCurvePoint { temp: 40, speed: 0 },
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_speed_over_100() {
        let config = Config {
            fan: FanConfig {
                curve: vec![FanCurvePoint {
                    temp: 40,
                    speed: 150,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_min_speed_over_100() {
        let config = Config {
            fan: FanConfig {
                min_speed_percent: 101,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_equal_temps() {
        let config = Config {
            fan: FanConfig {
                curve: vec![
                    FanCurvePoint { temp: 60, speed: 30 },
                    FanCurvePoint { temp: 60, speed: 50 },
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
