//! Clevo platform adapter — fan control via WMI/ACPI DSM kernel shim.
//!
//! All hardware access goes through sysfs at `/sys/devices/platform/tuxedo-clevo/`.
//! The kernel shim dispatches to either WMI or ACPI DSM transport.
//!
//! FANINFO u32 layout (returned by fan0_info / fan1_info / fan2_info):
//!   bits [7:0]   = fan duty (0–255)
//!   bits [15:8]  = temperature (°C)
//!   bits [31:16] = RPM
//!
//! fan_speed packed u32 (written to set speeds):
//!   bits [7:0]   = fan0 duty
//!   bits [15:8]  = fan1 duty
//!   bits [23:16] = fan2 duty

use crate::fan_curve::FanBackend;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

const SHIM_SYSFS_PATH: &str = "/sys/devices/platform/tuxedo-clevo";

/// Parsed FANINFO u32 from the kernel shim.
#[derive(Debug, Clone, Copy)]
struct FanInfo {
    duty: u8,
    temp: u8,
    rpm: u16,
}

impl FanInfo {
    fn parse(raw: u32) -> Self {
        Self {
            duty: (raw & 0xFF) as u8,
            temp: ((raw >> 8) & 0xFF) as u8,
            rpm: ((raw >> 16) & 0xFFFF) as u16,
        }
    }
}

pub struct ClevoPlatform {
    shim_path: PathBuf,
    num_fans: u8,
}

impl ClevoPlatform {
    pub fn init() -> io::Result<Self> {
        Self::init_with_path(Path::new(SHIM_SYSFS_PATH))
    }

    fn init_with_path(shim_path: &Path) -> io::Result<Self> {
        if !shim_path.join("fan0_info").exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Clevo shim not found at {}", shim_path.display()),
            ));
        }

        // Probe fan count: fan0 always exists, check fan1 and fan2
        let num_fans = Self::probe_fan_count(shim_path);

        info!(shim = %shim_path.display(), num_fans, "Clevo platform initialized");

        Ok(Self {
            shim_path: shim_path.to_path_buf(),
            num_fans,
        })
    }

    /// Probe number of fans by reading fan info attributes.
    /// fan0 always exists. fan1/fan2 may return 0 or error if not present.
    fn probe_fan_count(shim_path: &Path) -> u8 {
        let mut count = 1u8; // fan0 always present

        // fan1_info exists and returns a non-zero value → 2 fans
        if let Ok(info) = Self::read_fan_info_at(shim_path, 1) {
            if info.duty > 0 || info.temp > 0 || info.rpm > 0 {
                count = 2;
            }
        }

        // fan2_info exists and returns a non-zero value → 3 fans
        if count == 2 {
            if let Ok(info) = Self::read_fan_info_at(shim_path, 2) {
                if info.duty > 0 || info.temp > 0 || info.rpm > 0 {
                    count = 3;
                }
            }
        }

        count
    }

    fn write_attr(&self, attr: &str, value: &str) -> io::Result<()> {
        fs::write(self.shim_path.join(attr), value)
    }

    fn read_fan_info_at(shim_path: &Path, fan_index: u8) -> io::Result<FanInfo> {
        let attr = format!("fan{fan_index}_info");
        let raw: u32 = fs::read_to_string(shim_path.join(&attr))?
            .trim()
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(FanInfo::parse(raw))
    }

    fn read_fan_info(&self, fan_index: u8) -> io::Result<FanInfo> {
        Self::read_fan_info_at(&self.shim_path, fan_index)
    }

    pub fn shutdown(&self) {
        if let Err(e) = self.write_attr("fan_auto", "1") {
            tracing::warn!("failed to restore Clevo fans to auto: {e}");
        } else {
            info!("Clevo fans restored to auto mode");
        }
    }
}

impl FanBackend for ClevoPlatform {
    fn read_temp(&self) -> io::Result<u8> {
        let info = self.read_fan_info(0)?;
        Ok(info.temp)
    }

    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()> {
        // Read current state of all fans to preserve their speeds
        let mut speeds = [0u8; 3];
        for i in 0..self.num_fans {
            if let Ok(info) = self.read_fan_info(i) {
                speeds[i as usize] = info.duty;
            }
        }

        // Update the target fan
        if (fan_index as usize) < speeds.len() {
            speeds[fan_index as usize] = pwm;
        }

        // Pack into u32: fan0 | fan1<<8 | fan2<<16
        let packed: u32 = speeds[0] as u32
            | (speeds[1] as u32) << 8
            | (speeds[2] as u32) << 16;

        debug!(fan_index, pwm, packed, "writing Clevo fan speed");
        self.write_attr("fan_speed", &packed.to_string())
    }

    fn read_pwm(&self, fan_index: u8) -> io::Result<u8> {
        let info = self.read_fan_info(fan_index)?;
        Ok(info.duty)
    }

    fn set_auto(&self, _fan_index: u8) -> io::Result<()> {
        self.write_attr("fan_auto", "1")
    }

    fn num_fans(&self) -> u8 {
        self.num_fans
    }

    fn read_fan_rpm(&self, fan_index: u8) -> io::Result<u16> {
        let info = self.read_fan_info(fan_index)?;
        Ok(info.rpm)
    }
}

/// Check if the Clevo shim sysfs exists.
pub fn has_shim() -> bool {
    Path::new(SHIM_SYSFS_PATH).join("fan0_info").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_shim(dir: &Path, num_fans: u8) {
        fs::create_dir_all(dir).unwrap();
        // fan0: duty=128, temp=55, rpm=2800
        // packed: 2800<<16 | 55<<8 | 128 = 183521408 + 14080 + 128 = 183535616
        let fan0_info: u32 = 128 | (55 << 8) | (2800 << 16);
        fs::write(dir.join("fan0_info"), format!("{fan0_info}\n")).unwrap();

        if num_fans >= 2 {
            // fan1: duty=100, temp=50, rpm=2600
            let fan1_info: u32 = 100 | (50 << 8) | (2600 << 16);
            fs::write(dir.join("fan1_info"), format!("{fan1_info}\n")).unwrap();
        } else {
            // Write zero info to indicate fan not present
            fs::write(dir.join("fan1_info"), "0\n").unwrap();
        }

        // fan2: not present on most devices
        fs::write(dir.join("fan2_info"), "0\n").unwrap();

        fs::write(dir.join("fan_speed"), "0\n").unwrap();
        fs::write(dir.join("fan_auto"), "0\n").unwrap();
    }

    #[test]
    fn test_faninfo_parse() {
        // duty=128, temp=55, rpm=2800
        let raw: u32 = 128 | (55 << 8) | (2800 << 16);
        let info = FanInfo::parse(raw);
        assert_eq!(info.duty, 128);
        assert_eq!(info.temp, 55);
        assert_eq!(info.rpm, 2800);
    }

    #[test]
    fn test_faninfo_parse_zeros() {
        let info = FanInfo::parse(0);
        assert_eq!(info.duty, 0);
        assert_eq!(info.temp, 0);
        assert_eq!(info.rpm, 0);
    }

    #[test]
    fn test_faninfo_parse_max() {
        let raw: u32 = 255 | (255 << 8) | (0xFFFF << 16);
        let info = FanInfo::parse(raw);
        assert_eq!(info.duty, 255);
        assert_eq!(info.temp, 255);
        assert_eq!(info.rpm, 0xFFFF);
    }

    #[test]
    fn test_init_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-clevo");
        fake_shim(&shim_dir, 2);

        let platform = ClevoPlatform::init_with_path(&shim_dir).unwrap();
        assert_eq!(platform.num_fans(), 2);
        assert_eq!(platform.read_temp().unwrap(), 55);
        assert_eq!(platform.read_fan_rpm(0).unwrap(), 2800);
        assert_eq!(platform.read_fan_rpm(1).unwrap(), 2600);
        assert_eq!(platform.read_pwm(0).unwrap(), 128);
        assert_eq!(platform.read_pwm(1).unwrap(), 100);
    }

    #[test]
    fn test_write_pwm_packs_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-clevo");
        fake_shim(&shim_dir, 2);

        let platform = ClevoPlatform::init_with_path(&shim_dir).unwrap();

        // Write fan0=200, should read current fan1 duty (100) and pack
        platform.write_pwm(0, 200).unwrap();
        let written = fs::read_to_string(shim_dir.join("fan_speed")).unwrap();
        let packed: u32 = written.trim().parse().unwrap();

        // fan0=200, fan1=100, fan2=0
        assert_eq!(packed & 0xFF, 200);
        assert_eq!((packed >> 8) & 0xFF, 100);
        assert_eq!((packed >> 16) & 0xFF, 0);
    }

    #[test]
    fn test_set_auto() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-clevo");
        fake_shim(&shim_dir, 2);

        let platform = ClevoPlatform::init_with_path(&shim_dir).unwrap();
        platform.set_auto(0).unwrap();
        let written = fs::read_to_string(shim_dir.join("fan_auto")).unwrap();
        assert_eq!(written.trim(), "1");
    }

    #[test]
    fn test_init_missing_shim() {
        let dir = tempfile::tempdir().unwrap();
        let result = ClevoPlatform::init_with_path(&dir.path().join("nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn test_probe_fan_count_single() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-clevo");
        fake_shim(&shim_dir, 1);

        let platform = ClevoPlatform::init_with_path(&shim_dir).unwrap();
        assert_eq!(platform.num_fans(), 1);
    }
}
