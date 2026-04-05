//! Generic hwmon sysfs client.
//!
//! Reads/writes standard Linux hwmon attributes from `/sys/class/hwmon/hwmonN/`.
//! Works with any kernel driver that exposes hwmon (e.g. `uniwill-laptop`,
//! `thinkpad_acpi`, `coretemp`).

use crate::fan_curve::FanBackend;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Handle to a single hwmon sysfs device.
pub struct HwmonDevice {
    path: PathBuf,
    num_fans: u8,
}

impl HwmonDevice {
    /// Scan `/sys/class/hwmon/` and return the first device whose `name`
    /// attribute matches the given string.
    pub fn find_by_name(name: &str) -> io::Result<Self> {
        Self::find_by_name_in(Path::new("/sys/class/hwmon"), name)
    }

    /// Testable version that accepts a custom base path.
    fn find_by_name_in(base: &Path, name: &str) -> io::Result<Self> {
        for entry in fs::read_dir(base)? {
            let entry = entry?;
            let name_path = entry.path().join("name");
            if let Ok(content) = fs::read_to_string(&name_path) {
                if content.trim() == name {
                    debug!(path = %entry.path().display(), "found hwmon device");
                    let num_fans = Self::probe_fan_count(&entry.path());
                    return Ok(Self {
                        path: entry.path(),
                        num_fans,
                    });
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no hwmon device named '{name}'"),
        ))
    }

    /// Probe how many pwmN files exist (1-based: pwm1, pwm2, ...).
    fn probe_fan_count(path: &Path) -> u8 {
        let mut count = 0u8;
        for i in 1..=4 {
            if path.join(format!("pwm{i}")).exists() {
                count = i;
            } else {
                break;
            }
        }
        count
    }

    fn read_attr(&self, attr: &str) -> io::Result<String> {
        let path = self.path.join(attr);
        fs::read_to_string(&path).map(|s| s.trim().to_string())
    }

    fn write_attr(&self, attr: &str, value: &str) -> io::Result<()> {
        fs::write(self.path.join(attr), value)
    }

    /// Read `tempN_input` (millidegrees C) and return degrees C as u8.
    /// hwmon uses 1-based indexing.
    pub fn read_temp(&self, index: u8) -> io::Result<u8> {
        let millideg: i32 = self
            .read_attr(&format!("temp{index}_input"))?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok((millideg / 1000).clamp(0, 255) as u8)
    }

    /// Read `fanN_input` (RPM).
    pub fn read_fan_rpm(&self, index: u8) -> io::Result<u16> {
        self.read_attr(&format!("fan{index}_input"))?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Read `pwmN` (0–255).
    pub fn read_pwm(&self, index: u8) -> io::Result<u8> {
        self.read_attr(&format!("pwm{index}"))?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Write `pwmN` (0–255).
    pub fn write_pwm(&self, index: u8, pwm: u8) -> io::Result<()> {
        self.write_attr(&format!("pwm{index}"), &pwm.to_string())
    }

    /// Write `pwmN_enable` (1 = manual, 2 = auto).
    pub fn write_pwm_enable(&self, index: u8, mode: u8) -> io::Result<()> {
        self.write_attr(&format!("pwm{index}_enable"), &mode.to_string())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl FanBackend for HwmonDevice {
    fn read_temp(&self) -> io::Result<u8> {
        self.read_temp(1) // temp1_input = primary CPU temp
    }

    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()> {
        let idx = fan_index + 1; // 0-based → 1-based
        self.write_pwm_enable(idx, 1)?; // ensure manual mode
        self.write_pwm(idx, pwm)
    }

    fn read_pwm(&self, fan_index: u8) -> io::Result<u8> {
        self.read_pwm(fan_index + 1)
    }

    fn set_auto(&self, fan_index: u8) -> io::Result<()> {
        self.write_pwm_enable(fan_index + 1, 2)
    }

    fn num_fans(&self) -> u8 {
        self.num_fans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_hwmon(dir: &Path) {
        fs::write(dir.join("name"), "test_hwmon\n").unwrap();
        fs::write(dir.join("temp1_input"), "65000\n").unwrap();
        fs::write(dir.join("fan1_input"), "3200\n").unwrap();
        fs::write(dir.join("pwm1"), "128\n").unwrap();
        fs::write(dir.join("pwm1_enable"), "2\n").unwrap();
    }

    #[test]
    fn test_read_temp() {
        let dir = tempfile::tempdir().unwrap();
        let hwmon_dir = dir.path().join("hwmon0");
        fs::create_dir(&hwmon_dir).unwrap();
        fake_hwmon(&hwmon_dir);

        let dev = HwmonDevice::find_by_name_in(dir.path(), "test_hwmon").unwrap();
        assert_eq!(dev.read_temp(1).unwrap(), 65);
    }

    #[test]
    fn test_read_fan_rpm() {
        let dir = tempfile::tempdir().unwrap();
        let hwmon_dir = dir.path().join("hwmon0");
        fs::create_dir(&hwmon_dir).unwrap();
        fake_hwmon(&hwmon_dir);

        let dev = HwmonDevice::find_by_name_in(dir.path(), "test_hwmon").unwrap();
        assert_eq!(dev.read_fan_rpm(1).unwrap(), 3200);
    }

    #[test]
    fn test_read_write_pwm() {
        let dir = tempfile::tempdir().unwrap();
        let hwmon_dir = dir.path().join("hwmon0");
        fs::create_dir(&hwmon_dir).unwrap();
        fake_hwmon(&hwmon_dir);

        let dev = HwmonDevice::find_by_name_in(dir.path(), "test_hwmon").unwrap();
        assert_eq!(dev.read_pwm(1).unwrap(), 128);
        dev.write_pwm(1, 200).unwrap();
        assert_eq!(dev.read_pwm(1).unwrap(), 200);
    }

    #[test]
    fn test_fan_count() {
        let dir = tempfile::tempdir().unwrap();
        let hwmon_dir = dir.path().join("hwmon0");
        fs::create_dir(&hwmon_dir).unwrap();
        fake_hwmon(&hwmon_dir);

        let dev = HwmonDevice::find_by_name_in(dir.path(), "test_hwmon").unwrap();
        assert_eq!(dev.num_fans, 1);
    }

    #[test]
    fn test_find_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = HwmonDevice::find_by_name_in(dir.path(), "nonexistent");
        assert!(result.is_err());
    }
}
