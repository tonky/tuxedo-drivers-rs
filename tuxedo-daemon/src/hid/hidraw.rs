//! Low-level hidraw interface for ITE keyboard LED devices.
//!
//! Uses raw ioctls on /dev/hidraw* to send HID feature reports and
//! output reports, avoiding the libhidapi C dependency.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// HID device info from HIDIOCGRAWINFO ioctl.
#[derive(Debug, Clone)]
pub struct HidDeviceInfo {
    pub bus_type: u32,
    pub vendor_id: u16,
    pub product_id: u16,
    pub path: PathBuf,
}

/// Raw HID device handle.
pub struct HidrawDevice {
    file: File,
    pub info: HidDeviceInfo,
}

// ioctl definitions for hidraw
// HIDIOCGRAWINFO = _IOR('H', 0x03, struct hidraw_devinfo)
// struct hidraw_devinfo { __u32 bustype; __s16 vendor; __s16 product; }
nix::ioctl_read!(hidiocgrawinfo, b'H', 0x03, HidrawDevinfo);

// HIDIOCSFEATURE = _IOWR('H', 0x06, buf)
// HIDIOCGFEATURE = _IOWR('H', 0x07, buf)
nix::ioctl_read_buf!(hidiocgfeature, b'H', 0x07, u8);
nix::ioctl_write_buf!(hidiocsfeature, b'H', 0x06, u8);

#[repr(C)]
#[derive(Debug, Default)]
pub struct HidrawDevinfo {
    pub bustype: u32,
    pub vendor: i16,
    pub product: i16,
}

impl HidrawDevice {
    /// Open a hidraw device by path.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut devinfo = HidrawDevinfo::default();
        unsafe {
            hidiocgrawinfo(file.as_raw_fd(), &mut devinfo)
                .map_err(io::Error::other)?;
        }
        Ok(Self {
            file,
            info: HidDeviceInfo {
                bus_type: devinfo.bustype,
                vendor_id: devinfo.vendor as u16,
                product_id: devinfo.product as u16,
                path: path.to_path_buf(),
            },
        })
    }

    /// Send a HID feature report (SET_REPORT).
    pub fn send_feature_report(&self, data: &[u8]) -> io::Result<()> {
        unsafe {
            hidiocsfeature(self.file.as_raw_fd(), data)
                .map_err(io::Error::other)?;
        }
        Ok(())
    }

    /// Get a HID feature report (GET_REPORT).
    pub fn get_feature_report(&self, buf: &mut [u8]) -> io::Result<()> {
        unsafe {
            hidiocgfeature(self.file.as_raw_fd(), buf)
                .map_err(io::Error::other)?;
        }
        Ok(())
    }

    /// Send an output report (interrupt OUT endpoint).
    /// This is a regular write() on the hidraw fd.
    pub fn send_output_report(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)
    }
}

/// Enumerate all /dev/hidraw* devices.
pub fn enumerate() -> io::Result<Vec<HidDeviceInfo>> {
    let mut devices = Vec::new();
    let dev_dir = Path::new("/dev");

    let entries = fs::read_dir(dev_dir)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("hidraw") {
            continue;
        }

        let path = entry.path();
        match HidrawDevice::open(&path) {
            Ok(dev) => devices.push(dev.info),
            Err(_) => continue, // Permission denied or not a valid hidraw device
        }
    }

    Ok(devices)
}

/// Find all hidraw devices matching a specific vendor and product ID.
pub fn find_devices(vendor_id: u16, product_id: u16) -> io::Result<Vec<HidDeviceInfo>> {
    Ok(enumerate()?
        .into_iter()
        .filter(|d| d.vendor_id == vendor_id && d.product_id == product_id)
        .collect())
}

/// Read the bcdDevice value for a hidraw device from USB sysfs.
/// Returns None if not a USB device or sysfs attribute not found.
pub fn read_bcd_device(hidraw_path: &Path) -> Option<u16> {
    // /dev/hidrawN -> /sys/class/hidraw/hidrawN/device/../../bcdDevice
    let name = hidraw_path.file_name()?.to_str()?;
    let sysfs_bcd = format!("/sys/class/hidraw/{name}/device/../../bcdDevice");
    let bcd_str = fs::read_to_string(&sysfs_bcd).ok()?;
    u16::from_str_radix(bcd_str.trim().trim_start_matches("0x"), 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_bcd_device_nonexistent() {
        assert_eq!(read_bcd_device(Path::new("/dev/hidraw999")), None);
    }
}
