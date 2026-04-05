//! NB05 platform adapter — ties EC, fans, sensors, and backlight together.

pub mod fan;
pub mod kbd_backlight;
pub mod sensors;

use crate::dmi::{Nb05Data, TuxedoDevice};
use crate::ec::EcRam;
use crate::fan_curve::FanBackend;
use fan::FanController;
use kbd_backlight::KbdBacklight;
use std::io;
use tracing::{info, warn};

/// Complete NB05 hardware interface.
pub struct Nb05Platform {
    pub fan_ctl: FanController,
    pub kbd_bl: KbdBacklight,
    #[allow(dead_code)]
    pub product_sku: String,
}

impl Nb05Platform {
    /// Initialize the NB05 platform from a detected device.
    ///
    /// Opens the EC sysfs file, determines firmware version, and configures
    /// the fan controller with the correct write_rpm setting.
    pub fn init(device: &TuxedoDevice) -> io::Result<Self> {
        let nb05 = device
            .nb05_data
            .as_ref()
            .expect("Nb05Platform::init called for non-NB05 device");

        let ec = EcRam::open()?;

        let (fw_major, fw_minor) = ec.read_fw_version()?;
        info!(fw_major, fw_minor, "NB05 EC firmware version");

        let write_rpm = should_write_rpm(nb05, fw_major, fw_minor);
        if write_rpm {
            info!("RPM register writes enabled (firmware < 9.10)");
        }

        let fan_ctl = FanController::new(ec, nb05.num_fans, nb05.fanctl_onereg, write_rpm);
        let kbd_bl = KbdBacklight::new(&device.product_sku);

        Ok(Self {
            fan_ctl,
            kbd_bl,
            product_sku: device.product_sku.clone(),
        })
    }

    /// Restore all fans to auto mode and log any errors.
    pub fn shutdown(&self) {
        if let Err(e) = self.fan_ctl.restore_auto() {
            warn!("failed to restore fans to auto on shutdown: {e}");
        } else {
            info!("fans restored to auto mode");
        }
    }
}

impl FanBackend for Nb05Platform {
    fn read_temp(&self) -> io::Result<u8> {
        sensors::read_cpu_temp(self.fan_ctl.ec())
    }

    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()> {
        self.fan_ctl.set_fan_pwm(fan_index, pwm)
    }

    fn read_pwm(&self, fan_index: u8) -> io::Result<u8> {
        self.fan_ctl.get_fan_pwm(fan_index)
    }

    fn set_auto(&self, fan_index: u8) -> io::Result<()> {
        self.fan_ctl.set_fan_auto(fan_index)
    }

    fn read_fan_rpm(&self, fan_index: u8) -> io::Result<u16> {
        match fan_index {
            0 => sensors::read_fan1_rpm(self.fan_ctl.ec()),
            1 => sensors::read_fan2_rpm(self.fan_ctl.ec()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid fan index {fan_index}"),
            )),
        }
    }

    fn num_fans(&self) -> u8 {
        self.fan_ctl.num_fans()
    }
}

/// Determine whether RPM registers should be written.
/// Only for non-onereg devices with firmware < 9.10.
fn should_write_rpm(nb05: &Nb05Data, fw_major: u8, fw_minor: u8) -> bool {
    if nb05.fanctl_onereg {
        return false;
    }
    // Firmware >= 9.10 manages RPM tables itself
    // Compare as a composite version number to handle major > 9 correctly
    let fw_ver = (fw_major as u16) * 100 + fw_minor as u16;
    fw_ver < 9 * 100 + 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dmi::Nb05Data;

    #[test]
    fn test_should_write_rpm() {
        let pulse = Nb05Data {
            num_fans: 2,
            fanctl_onereg: false,
        };
        let iflex = Nb05Data {
            num_fans: 1,
            fanctl_onereg: true,
        };

        // Old firmware on Pulse → write RPM
        assert!(should_write_rpm(&pulse, 9, 9));
        assert!(should_write_rpm(&pulse, 8, 15));

        // New firmware on Pulse → don't write RPM
        assert!(!should_write_rpm(&pulse, 9, 10));
        assert!(!should_write_rpm(&pulse, 10, 0));

        // InfinityFlex → never write RPM
        assert!(!should_write_rpm(&iflex, 8, 0));
        assert!(!should_write_rpm(&iflex, 9, 9));
    }
}
