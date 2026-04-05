//! Uniwill platform adapter — self-contained EC access via `tuxedo-uw-fan` shim.
//!
//! All hardware access goes through the `tuxedo-uw-fan` kernel module's sysfs at
//! `/sys/devices/platform/tuxedo-uw-fan/`. No dependency on upstream `uniwill-laptop`.
//!
//! sysfs attributes used:
//! - `fan0_pwm`, `fan1_pwm` (RW) — fan duty 0–200 (EC scale)
//! - `fan_mode` (RW) — 0 = auto, 1 = manual
//! - `cpu_temp` (R) — CPU temperature in degrees C
//! - `gpu_temp` (R) — GPU temperature in degrees C
//! - `fan_count` (R) — number of fans

use crate::fan_curve::FanBackend;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// PWM maximum in EC terms (0–200). Daemon uses 0–255.
const EC_PWM_MAX: u16 = 200;

/// Path to the shim's sysfs directory.
const SHIM_SYSFS_PATH: &str = "/sys/devices/platform/tuxedo-uw-fan";

/// Uniwill platform — reads and writes everything via the kernel shim.
pub struct UniwillPlatform {
    shim_path: PathBuf,
    num_fans: u8,
}

impl UniwillPlatform {
    /// Initialize by verifying the shim sysfs exists.
    pub fn init() -> io::Result<Self> {
        Self::init_with_path(Path::new(SHIM_SYSFS_PATH))
    }

    /// Testable version with custom shim path.
    fn init_with_path(shim_path: &Path) -> io::Result<Self> {
        if !shim_path.join("fan_mode").exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("shim not found at {}", shim_path.display()),
            ));
        }

        let num_fans = Self::read_attr_parsed(shim_path, "fan_count").unwrap_or(2);

        info!(
            shim = %shim_path.display(),
            num_fans,
            "Uniwill platform initialized"
        );

        Ok(Self {
            shim_path: shim_path.to_path_buf(),
            num_fans,
        })
    }

    fn read_attr(&self, attr: &str) -> io::Result<String> {
        let path = self.shim_path.join(attr);
        fs::read_to_string(&path).map(|s| s.trim().to_string())
    }

    fn read_attr_parsed<T: std::str::FromStr>(base: &Path, attr: &str) -> io::Result<T> {
        let path = base.join(attr);
        let s = fs::read_to_string(&path)?;
        s.trim()
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("parse error: {attr}")))
    }

    fn write_attr(&self, attr: &str, value: &str) -> io::Result<()> {
        fs::write(self.shim_path.join(attr), value)
    }

    #[allow(dead_code)]
    fn read_shim_pwm(&self, fan_index: u8) -> io::Result<u8> {
        self.read_attr(&format!("fan{fan_index}_pwm"))?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn write_shim_pwm(&self, fan_index: u8, ec_pwm: u8) -> io::Result<()> {
        self.write_attr(&format!("fan{fan_index}_pwm"), &ec_pwm.to_string())
    }

    fn write_fan_mode(&self, manual: bool) -> io::Result<()> {
        self.write_attr("fan_mode", if manual { "1" } else { "0" })
    }

    /// Restore fans to auto mode. Called on shutdown.
    pub fn shutdown(&self) {
        if let Err(e) = self.write_fan_mode(false) {
            tracing::warn!("failed to restore Uniwill fans to auto: {e}");
        } else {
            info!("Uniwill fans restored to auto mode");
        }
    }
}

/// Scale 0–255 (daemon) to 0–200 (EC).
fn pwm_to_ec(pwm: u8) -> u8 {
    ((pwm as u16 * EC_PWM_MAX) / 255) as u8
}

#[allow(dead_code)]
fn ec_to_pwm(ec: u8) -> u8 {
    ((ec as u16 * 255) / EC_PWM_MAX) as u8
}

impl FanBackend for UniwillPlatform {
    fn read_temp(&self) -> io::Result<u8> {
        self.read_attr("cpu_temp")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()> {
        self.write_fan_mode(true)?;
        let ec_pwm = pwm_to_ec(pwm);
        debug!(fan_index, pwm, ec_pwm, "writing Uniwill fan PWM");
        self.write_shim_pwm(fan_index, ec_pwm)
    }

    fn read_pwm(&self, fan_index: u8) -> io::Result<u8> {
        let ec_pwm = self.read_shim_pwm(fan_index)?;
        Ok(ec_to_pwm(ec_pwm))
    }

    fn set_auto(&self, _fan_index: u8) -> io::Result<()> {
        self.write_fan_mode(false)
    }

    fn num_fans(&self) -> u8 {
        self.num_fans
    }

    fn read_fan_rpm(&self, _fan_index: u8) -> io::Result<u16> {
        // Uniwill EC has no dedicated RPM registers; duty-only control.
        // RPM reads are not available without the upstream uniwill-laptop driver.
        Ok(0)
    }
}

/// Check if the shim sysfs exists.
pub fn has_fan_shim() -> bool {
    Path::new(SHIM_SYSFS_PATH).join("fan_mode").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_shim(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("fan0_pwm"), "100\n").unwrap();
        fs::write(dir.join("fan1_pwm"), "100\n").unwrap();
        fs::write(dir.join("fan_mode"), "0\n").unwrap();
        fs::write(dir.join("cpu_temp"), "55\n").unwrap();
        fs::write(dir.join("gpu_temp"), "42\n").unwrap();
        fs::write(dir.join("fan_count"), "2\n").unwrap();
    }

    #[test]
    fn test_pwm_scaling() {
        assert_eq!(pwm_to_ec(0), 0);
        assert_eq!(pwm_to_ec(255), 200);
        assert_eq!(pwm_to_ec(127), 99);

        assert_eq!(ec_to_pwm(0), 0);
        assert_eq!(ec_to_pwm(200), 255);
        assert_eq!(ec_to_pwm(100), 127);
    }

    #[test]
    fn test_round_trip() {
        for pwm in [0u8, 50, 100, 127, 255] {
            let ec = pwm_to_ec(pwm);
            let back = ec_to_pwm(ec);
            assert!(
                (back as i16 - pwm as i16).unsigned_abs() <= 1,
                "round trip failed: {pwm} → {ec} → {back}"
            );
        }
    }

    #[test]
    fn test_init_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-uw-fan");
        fake_shim(&shim_dir);

        let platform = UniwillPlatform::init_with_path(&shim_dir).unwrap();
        assert_eq!(platform.num_fans(), 2);
        assert_eq!(platform.read_temp().unwrap(), 55);
        assert_eq!(platform.read_fan_rpm(0).unwrap(), 0); // not available
        assert_eq!(platform.read_pwm(0).unwrap(), 127); // 100 EC → 127

        // Write PWM
        platform.write_pwm(0, 255).unwrap();
        let written = fs::read_to_string(shim_dir.join("fan0_pwm")).unwrap();
        assert_eq!(written.trim(), "200");
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
        let result = UniwillPlatform::init_with_path(&dir.path().join("nonexistent"));
        assert!(result.is_err());
    }
}
