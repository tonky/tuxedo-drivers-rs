//! HID device discovery — find ITE keyboard LED controllers.

use super::color_scaling::ColorScaling;
use super::hidraw::{self, HidrawDevice};
use super::KeyboardLed;
use super::{ite8291, ite8291_lb, ite8297, ite829x};
use tracing::{debug, info, warn};

const ITE_VID: u16 = 0x048d;

/// Discovered ITE LED device, ready to be opened.
#[derive(Debug)]
pub struct DiscoveredDevice {
    pub path: std::path::PathBuf,
    pub product_id: u16,
    pub device_type: IteDeviceType,
}

#[derive(Debug, Clone, Copy)]
pub enum IteDeviceType {
    Ite8291,
    Ite8291Lb,
    Ite8297,
    Ite829x,
}

/// Scan for all ITE keyboard LED devices.
pub fn scan() -> Vec<DiscoveredDevice> {
    let devices = match hidraw::enumerate() {
        Ok(devs) => devs,
        Err(e) => {
            warn!("failed to enumerate hidraw devices: {e}");
            return Vec::new();
        }
    };

    let mut found = Vec::new();

    for dev in devices {
        if dev.vendor_id != ITE_VID {
            continue;
        }

        let device_type = match dev.product_id {
            pid if ite8291::PIDS.contains(&pid) => IteDeviceType::Ite8291,
            pid if ite8291_lb::PIDS.contains(&pid) => IteDeviceType::Ite8291Lb,
            ite8297::PID => IteDeviceType::Ite8297,
            ite829x::PID => IteDeviceType::Ite829x,
            _ => continue,
        };

        debug!(
            path = %dev.path.display(),
            product_id = format!("0x{:04x}", dev.product_id),
            device_type = ?device_type,
            "found ITE LED device"
        );

        found.push(DiscoveredDevice {
            path: dev.path,
            product_id: dev.product_id,
            device_type,
        });
    }

    info!("found {} ITE LED device(s)", found.len());
    found
}

/// Open a discovered device and return a KeyboardLed implementation.
pub fn open_device(
    discovered: &DiscoveredDevice,
    color_scaling: ColorScaling,
) -> std::io::Result<Box<dyn KeyboardLed>> {
    let device = HidrawDevice::open(&discovered.path)?;

    match discovered.device_type {
        IteDeviceType::Ite8291 => {
            let bcd = hidraw::read_bcd_device(&discovered.path).unwrap_or(0);
            let ctrl = ite8291::Ite8291::new(device, bcd, color_scaling)?;
            Ok(Box::new(ctrl))
        }
        IteDeviceType::Ite8291Lb => {
            let ctrl = ite8291_lb::Ite8291Lb::new(device, color_scaling)?;
            Ok(Box::new(ctrl))
        }
        IteDeviceType::Ite8297 => {
            let ctrl = ite8297::Ite8297::new(device)?;
            Ok(Box::new(ctrl))
        }
        IteDeviceType::Ite829x => {
            let ctrl = ite829x::Ite829x::new(device)?;
            Ok(Box::new(ctrl))
        }
    }
}
