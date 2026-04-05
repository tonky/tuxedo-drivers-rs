//! NB05 temperature and RPM sensors — port of `tuxedo_nb05_sensors.c`.
//!
//! Note: the original C driver has a copy-paste bug in `read_fan1_rpm()` —
//! it reads 0x0298 for both high and low bytes.  We fix this: fan 1 uses
//! 0x0298 (high) / 0x0299 (low), matching the fan 2 pattern at 0x0218/0x0219.

use crate::ec::EcRam;
use std::io;

/// Read CPU temperature in degrees Celsius.
pub fn read_cpu_temp(ec: &EcRam) -> io::Result<u8> {
    ec.read_byte(0x0470)
}

/// Read fan 1 RPM (EC registers 0x0298 high, 0x0299 low).
///
/// The original C driver reads 0x0298 twice (bug). We use 0x0299 for the
/// low byte, matching the fan 2 register layout.
pub fn read_fan1_rpm(ec: &EcRam) -> io::Result<u16> {
    let high = ec.read_byte(0x0298)? as u16;
    let low = ec.read_byte(0x0299)? as u16;
    Ok((high << 8) | low)
}

/// Read fan 2 RPM (EC registers 0x0218 high, 0x0219 low).
pub fn read_fan2_rpm(ec: &EcRam) -> io::Result<u16> {
    let high = ec.read_byte(0x0218)? as u16;
    let low = ec.read_byte(0x0219)? as u16;
    Ok((high << 8) | low)
}

#[allow(dead_code)]
pub struct FanLimits {
    pub min_rpm: u16,
    pub max_rpm: u16,
}

#[allow(dead_code)]
pub fn fan_limits(product_sku: &str) -> FanLimits {
    match product_sku {
        "IFLX14I01" => FanLimits {
            min_rpm: 0,
            max_rpm: 5600,
        },
        _ => FanLimits {
            min_rpm: 0,
            max_rpm: 5400,
        },
    }
}
