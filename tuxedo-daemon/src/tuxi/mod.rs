//! Tuxi platform adapter — fan control via `tuxedo-tuxi` kernel shim.
//!
//! All hardware access goes through sysfs at `/sys/devices/platform/tuxedo-tuxi/`.
//! The kernel shim calls ACPI TFAN methods (SSPD, GSPD, GCNT, SMOD, GMOD, GTMP, GRPM).
//!
//! Temperature is returned from ACPI in tenth-Kelvin; we convert to degrees C.
//! PWM range is 0–255 natively (no scaling needed).

use crate::fan_curve::FanBackend;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

const SHIM_SYSFS_PATH: &str = "/sys/devices/platform/tuxedo-tuxi";

pub struct TuxiPlatform {
    shim_path: PathBuf,
    num_fans: u8,
}

impl TuxiPlatform {
    pub fn init() -> io::Result<Self> {
        Self::init_with_path(Path::new(SHIM_SYSFS_PATH))
    }

    fn init_with_path(shim_path: &Path) -> io::Result<Self> {
        if !shim_path.join("fan_mode").exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Tuxi shim not found at {}", shim_path.display()),
            ));
        }

        let num_fans = Self::read_parsed(shim_path, "fan_count").unwrap_or(2);

        info!(shim = %shim_path.display(), num_fans, "Tuxi platform initialized");

        Ok(Self {
            shim_path: shim_path.to_path_buf(),
            num_fans,
        })
    }

    fn read_attr(&self, attr: &str) -> io::Result<String> {
        fs::read_to_string(self.shim_path.join(attr)).map(|s| s.trim().to_string())
    }

    fn read_parsed<T: std::str::FromStr>(base: &Path, attr: &str) -> io::Result<T> {
        fs::read_to_string(base.join(attr))?
            .trim()
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("parse error: {attr}")))
    }

    fn write_attr(&self, attr: &str, value: &str) -> io::Result<()> {
        fs::write(self.shim_path.join(attr), value)
    }

    fn write_fan_mode(&self, manual: bool) -> io::Result<()> {
        self.write_attr("fan_mode", if manual { "1" } else { "0" })
    }

    /// Convert tenth-Kelvin to degrees Celsius.
    fn tenth_kelvin_to_celsius(tenth_k: u16) -> u8 {
        let celsius = (tenth_k as i32 - 2730) / 10;
        celsius.clamp(0, 255) as u8
    }

    pub fn shutdown(&self) {
        if let Err(e) = self.write_fan_mode(false) {
            tracing::warn!("failed to restore Tuxi fans to auto: {e}");
        } else {
            info!("Tuxi fans restored to auto mode");
        }
    }
}

impl FanBackend for TuxiPlatform {
    fn read_temp(&self) -> io::Result<u8> {
        let raw: u16 = self.read_attr("fan0_temp")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Self::tenth_kelvin_to_celsius(raw))
    }

    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()> {
        self.write_fan_mode(true)?;
        debug!(fan_index, pwm, "writing Tuxi fan PWM");
        self.write_attr(&format!("fan{fan_index}_pwm"), &pwm.to_string())
    }

    fn read_pwm(&self, fan_index: u8) -> io::Result<u8> {
        self.read_attr(&format!("fan{fan_index}_pwm"))?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn set_auto(&self, _fan_index: u8) -> io::Result<()> {
        self.write_fan_mode(false)
    }

    fn num_fans(&self) -> u8 {
        self.num_fans
    }

    fn read_fan_rpm(&self, fan_index: u8) -> io::Result<u16> {
        self.read_attr(&format!("fan{fan_index}_rpm"))?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

/// Check if the Tuxi shim sysfs exists.
pub fn has_shim() -> bool {
    Path::new(SHIM_SYSFS_PATH).join("fan_mode").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_shim(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("fan_count"), "2\n").unwrap();
        fs::write(dir.join("fan_mode"), "0\n").unwrap();
        fs::write(dir.join("fan0_pwm"), "128\n").unwrap();
        fs::write(dir.join("fan1_pwm"), "100\n").unwrap();
        fs::write(dir.join("fan0_temp"), "3230\n").unwrap(); // 50°C in tenth-K
        fs::write(dir.join("fan1_temp"), "3180\n").unwrap(); // 45°C
        fs::write(dir.join("fan0_rpm"), "2800\n").unwrap();
        fs::write(dir.join("fan1_rpm"), "2600\n").unwrap();
    }

    #[test]
    fn test_tenth_kelvin_to_celsius() {
        assert_eq!(TuxiPlatform::tenth_kelvin_to_celsius(2730), 0);
        assert_eq!(TuxiPlatform::tenth_kelvin_to_celsius(3230), 50);
        assert_eq!(TuxiPlatform::tenth_kelvin_to_celsius(3730), 100);
        assert_eq!(TuxiPlatform::tenth_kelvin_to_celsius(2930), 20);
    }

    #[test]
    fn test_init_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-tuxi");
        fake_shim(&shim_dir);

        let platform = TuxiPlatform::init_with_path(&shim_dir).unwrap();
        assert_eq!(platform.num_fans(), 2);
        assert_eq!(platform.read_temp().unwrap(), 50);
        assert_eq!(platform.read_fan_rpm(0).unwrap(), 2800);
        assert_eq!(platform.read_fan_rpm(1).unwrap(), 2600);
        assert_eq!(platform.read_pwm(0).unwrap(), 128);

        // Write PWM (native 0-255, no scaling)
        platform.write_pwm(0, 200).unwrap();
        let written = fs::read_to_string(shim_dir.join("fan0_pwm")).unwrap();
        assert_eq!(written.trim(), "200");
        // Fan mode should be manual
        let mode = fs::read_to_string(shim_dir.join("fan_mode")).unwrap();
        assert_eq!(mode.trim(), "1");

        // Set auto
        platform.set_auto(0).unwrap();
        let mode = fs::read_to_string(shim_dir.join("fan_mode")).unwrap();
        assert_eq!(mode.trim(), "0");
    }

    #[test]
    fn test_init_missing_shim() {
        let dir = tempfile::tempdir().unwrap();
        let result = TuxiPlatform::init_with_path(&dir.path().join("nonexistent"));
        assert!(result.is_err());
    }
}
