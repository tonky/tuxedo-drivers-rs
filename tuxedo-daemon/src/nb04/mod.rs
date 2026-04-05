//! NB04 platform adapter — sensors and power profiles via WMI kernel shim.
//!
//! All hardware access goes through sysfs at `/sys/devices/platform/tuxedo-nb04/`.
//! The kernel shim calls WMI BS methods (GUID 1F174999-...).
//!
//! NB04 has NO direct fan PWM control — fans are governed by profile selection
//! (battery/balanced/performance). This adapter does NOT implement FanBackend.
//!
//! sysfs attributes:
//!   cpu_temp        (RO) - degrees C
//!   gpu_temp        (RO) - degrees C
//!   fan0_rpm        (RO) - CPU fan RPM
//!   fan1_rpm        (RO) - GPU fan RPM
//!   power_profile   (RW) - 0=battery, 1=balanced, 2=performance

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use tracing::info;

const SHIM_SYSFS_PATH: &str = "/sys/devices/platform/tuxedo-nb04";

/// Power profile modes for NB04 devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Battery = 0,
    Balanced = 1,
    Performance = 2,
}

impl PowerProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Battery => "battery",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "battery" | "low-power" => Some(Self::Battery),
            "balanced" | "human" => Some(Self::Balanced),
            "performance" | "beast" => Some(Self::Performance),
            _ => None,
        }
    }
}

pub struct Nb04Platform {
    shim_path: PathBuf,
    /// Cached last-written profile (firmware has no read-back).
    current_profile: std::sync::Mutex<PowerProfile>,
}

impl Nb04Platform {
    pub fn init() -> io::Result<Self> {
        Self::init_with_path(Path::new(SHIM_SYSFS_PATH))
    }

    fn init_with_path(shim_path: &Path) -> io::Result<Self> {
        if !shim_path.join("cpu_temp").exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("NB04 shim not found at {}", shim_path.display()),
            ));
        }

        info!(shim = %shim_path.display(), "NB04 platform initialized");

        Ok(Self {
            shim_path: shim_path.to_path_buf(),
            current_profile: std::sync::Mutex::new(PowerProfile::Performance),
        })
    }

    #[allow(dead_code)]
    fn read_attr(&self, attr: &str) -> io::Result<String> {
        fs::read_to_string(self.shim_path.join(attr)).map(|s| s.trim().to_string())
    }

    fn write_attr(&self, attr: &str, value: &str) -> io::Result<()> {
        fs::write(self.shim_path.join(attr), value)
    }

    #[allow(dead_code)]
    pub fn read_cpu_temp(&self) -> io::Result<u8> {
        self.read_attr("cpu_temp")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    #[allow(dead_code)]
    pub fn read_gpu_temp(&self) -> io::Result<u8> {
        self.read_attr("gpu_temp")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    #[allow(dead_code)]
    pub fn read_fan0_rpm(&self) -> io::Result<u16> {
        self.read_attr("fan0_rpm")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    #[allow(dead_code)]
    pub fn read_fan1_rpm(&self) -> io::Result<u16> {
        self.read_attr("fan1_rpm")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Set power profile. Updates cached value on success.
    pub fn set_profile(&self, profile: PowerProfile) -> io::Result<()> {
        self.write_attr("power_profile", &(profile as u8).to_string())?;
        if let Ok(mut cached) = self.current_profile.lock() {
            *cached = profile;
        }
        info!(profile = profile.as_str(), "NB04 power profile set");
        Ok(())
    }

    #[allow(dead_code)]
    pub fn current_profile(&self) -> PowerProfile {
        self.current_profile
            .lock()
            .map(|guard| *guard)
            .unwrap_or(PowerProfile::Performance)
    }

    #[allow(dead_code)]
    pub fn num_fans(&self) -> u8 {
        2
    }
}

/// Check if the NB04 shim sysfs exists.
pub fn has_shim() -> bool {
    Path::new(SHIM_SYSFS_PATH).join("cpu_temp").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_shim(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("cpu_temp"), "65\n").unwrap();
        fs::write(dir.join("gpu_temp"), "58\n").unwrap();
        fs::write(dir.join("fan0_rpm"), "3200\n").unwrap();
        fs::write(dir.join("fan1_rpm"), "2900\n").unwrap();
        fs::write(dir.join("power_profile"), "-1\n").unwrap();
    }

    #[test]
    fn test_init_and_read_sensors() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-nb04");
        fake_shim(&shim_dir);

        let platform = Nb04Platform::init_with_path(&shim_dir).unwrap();
        assert_eq!(platform.read_cpu_temp().unwrap(), 65);
        assert_eq!(platform.read_gpu_temp().unwrap(), 58);
        assert_eq!(platform.read_fan0_rpm().unwrap(), 3200);
        assert_eq!(platform.read_fan1_rpm().unwrap(), 2900);
        assert_eq!(platform.num_fans(), 2);
    }

    #[test]
    fn test_set_profile() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().join("tuxedo-nb04");
        fake_shim(&shim_dir);

        let platform = Nb04Platform::init_with_path(&shim_dir).unwrap();

        // Default is Performance
        assert_eq!(platform.current_profile(), PowerProfile::Performance);

        // Set to Battery
        platform.set_profile(PowerProfile::Battery).unwrap();
        assert_eq!(platform.current_profile(), PowerProfile::Battery);
        let written = fs::read_to_string(shim_dir.join("power_profile")).unwrap();
        assert_eq!(written.trim(), "0");

        // Set to Balanced
        platform.set_profile(PowerProfile::Balanced).unwrap();
        assert_eq!(platform.current_profile(), PowerProfile::Balanced);
        let written = fs::read_to_string(shim_dir.join("power_profile")).unwrap();
        assert_eq!(written.trim(), "1");
    }

    #[test]
    fn test_power_profile_from_str() {
        assert_eq!(PowerProfile::from_str("battery"), Some(PowerProfile::Battery));
        assert_eq!(PowerProfile::from_str("low-power"), Some(PowerProfile::Battery));
        assert_eq!(PowerProfile::from_str("balanced"), Some(PowerProfile::Balanced));
        assert_eq!(PowerProfile::from_str("human"), Some(PowerProfile::Balanced));
        assert_eq!(PowerProfile::from_str("performance"), Some(PowerProfile::Performance));
        assert_eq!(PowerProfile::from_str("beast"), Some(PowerProfile::Performance));
        assert_eq!(PowerProfile::from_str("invalid"), None);
    }

    #[test]
    fn test_init_missing_shim() {
        let dir = tempfile::tempdir().unwrap();
        let result = Nb04Platform::init_with_path(&dir.path().join("nonexistent"));
        assert!(result.is_err());
    }
}
