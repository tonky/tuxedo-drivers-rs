//! NB05 fan control — port of `tuxedo_nb05_fan_control.c`.
//!
//! Supports two variants:
//! - **ranges** (Pulse 14): 9 temperature-indexed duty registers per fan,
//!   plus parallel RPM registers (firmware < 9.10 only).
//! - **onereg** (InfinityFlex): single duty register at 0x1809, enable at 0x02f1.
//!
//! Fan speed values use a deadband: ≤12.5% → off, 12.5%–25% → snap to 25%.
//! The top 2 temperature registers enforce a high-temp safety floor.

use crate::ec::EcRam;
use std::io;

// --- Constants (matching the C driver exactly) ---

const FAN_SET_RPM_MAX: u8 = 54;
const FAN_SET_DUTY_MAX: u16 = 0xb8; // 184
const FAN_ON_MIN_SPEED_PERCENT: u16 = 25;
const FAN_SET_RPM_HIGHTEMP: u8 = 15;
const FAN_SET_DUTY_HIGHTEMP: u16 =
    (FAN_SET_RPM_HIGHTEMP as u16 * FAN_SET_DUTY_MAX) / FAN_SET_RPM_MAX as u16;

/// Convert a 0–255 PWM value to the EC's duty scale (0–0xb8).
fn pwm_to_duty(pwm: u8) -> u16 {
    (pwm as u16 * FAN_SET_DUTY_MAX) / 255
}

/// Convert an EC duty value back to 0–255 PWM.
fn duty_to_pwm(duty: u8) -> u8 {
    ((duty as u16 * 255) / FAN_SET_DUTY_MAX) as u8
}

/// Convert a 0–255 PWM value to the EC's RPM scale (0–54).
fn pwm_to_rpm(pwm: u8) -> u8 {
    ((pwm as u16 * FAN_SET_RPM_MAX as u16) / 255) as u8
}

/// Apply the deadband: values ≤12.5% snap to 0, values 12.5%–25% snap up to 25%.
fn apply_deadband(val: u16, max: u16) -> u16 {
    let half_min = FAN_ON_MIN_SPEED_PERCENT * max / 2 / 100; // 12.5%
    let min_on = FAN_ON_MIN_SPEED_PERCENT * max / 100;        // 25%

    if val <= half_min {
        0
    } else if val < min_on {
        min_on
    } else {
        val
    }
}

/// Fan controller state for a single NB05 device.
pub struct FanController {
    ec: EcRam,
    /// Whether to also write RPM registers (firmware < 9.10, non-onereg only).
    write_rpm: bool,
    /// Number of fans on this device (1 or 2).
    num_fans: u8,
    /// Whether this device uses single-register fan control (InfinityFlex).
    onereg: bool,
}

impl FanController {
    /// Create a new fan controller.
    ///
    /// `write_rpm` should be `true` only for non-onereg devices with EC firmware < 9.10.
    pub fn new(ec: EcRam, num_fans: u8, onereg: bool, write_rpm: bool) -> Self {
        Self {
            ec,
            write_rpm,
            num_fans,
            onereg,
        }
    }

    pub fn num_fans(&self) -> u8 {
        self.num_fans
    }

    pub fn ec(&self) -> &EcRam {
        &self.ec
    }

    // --- Fan 1 ---

    /// Read fan 1 duty as PWM 0–255.
    pub fn read_fan1_pwm(&self) -> io::Result<u8> {
        let duty = if self.onereg {
            self.ec.read_byte(0x1809)?
        } else {
            self.ec.read_byte(0x02c1)?
        };
        Ok(duty_to_pwm(duty))
    }

    /// Set fan 1 speed from PWM 0–255.
    pub fn write_fan1_pwm(&self, pwm: u8) -> io::Result<()> {
        let duty = apply_deadband(pwm_to_duty(pwm), FAN_SET_DUTY_MAX) as u8;

        if self.onereg {
            self.ec.write_byte(0x1809, duty)?;
        } else {
            // Write 7 low-temp registers (0x02c1–0x02c7)
            for reg in 0x02c1..=0x02c7 {
                self.ec.write_byte(reg, duty)?;
            }
            // High-temp floor for top 2 registers (0x02c8–0x02c9)
            let hi_duty = if (duty as u16) < FAN_SET_DUTY_HIGHTEMP {
                FAN_SET_DUTY_HIGHTEMP as u8
            } else {
                duty
            };
            self.ec.write_byte(0x02c8, hi_duty)?;
            self.ec.write_byte(0x02c9, hi_duty)?;
        }

        // Write RPM registers if needed (non-onereg, firmware < 9.10)
        if self.write_rpm {
            let rpm = apply_deadband(pwm_to_rpm(pwm) as u16, FAN_SET_RPM_MAX as u16) as u8;
            for reg in 0x02d0..=0x02d6 {
                self.ec.write_byte(reg, rpm)?;
            }
            let hi_rpm = rpm.max(FAN_SET_RPM_HIGHTEMP);
            self.ec.write_byte(0x02d7, hi_rpm)?;
            self.ec.write_byte(0x02d8, hi_rpm)?;
        }

        Ok(())
    }

    /// Read fan 1 manual enable state.
    pub fn read_fan1_enable(&self) -> io::Result<bool> {
        if self.onereg {
            Ok(self.ec.read_byte(0x02f1)? == 0xaa)
        } else {
            Ok(self.ec.read_byte(0x02c0)? & 0x01 != 0)
        }
    }

    /// Set fan 1 manual/auto mode. `true` = manual, `false` = auto.
    pub fn write_fan1_enable(&self, enable: bool) -> io::Result<()> {
        if self.onereg {
            self.ec.write_byte(0x02f1, if enable { 0xaa } else { 0x00 })
        } else {
            self.ec.write_byte(0x02c0, if enable { 0x01 } else { 0x00 })
        }
    }

    // --- Fan 2 (only ranges-variant, never onereg) ---

    /// Read fan 2 duty as PWM 0–255.
    pub fn read_fan2_pwm(&self) -> io::Result<u8> {
        if self.num_fans < 2 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no fan 2"));
        }
        let duty = self.ec.read_byte(0x0241)?;
        Ok(duty_to_pwm(duty))
    }

    /// Set fan 2 speed from PWM 0–255.
    pub fn write_fan2_pwm(&self, pwm: u8) -> io::Result<()> {
        if self.num_fans < 2 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no fan 2"));
        }
        let duty = apply_deadband(pwm_to_duty(pwm), FAN_SET_DUTY_MAX) as u8;

        // 7 low-temp registers (0x0241–0x0247)
        for reg in 0x0241..=0x0247 {
            self.ec.write_byte(reg, duty)?;
        }
        // High-temp floor (0x0248–0x0249)
        let hi_duty = if (duty as u16) < FAN_SET_DUTY_HIGHTEMP {
            FAN_SET_DUTY_HIGHTEMP as u8
        } else {
            duty
        };
        self.ec.write_byte(0x0248, hi_duty)?;
        self.ec.write_byte(0x0249, hi_duty)?;

        // RPM registers for fan 2
        if self.write_rpm {
            let rpm = apply_deadband(pwm_to_rpm(pwm) as u16, FAN_SET_RPM_MAX as u16) as u8;
            for reg in 0x0250..=0x0256 {
                self.ec.write_byte(reg, rpm)?;
            }
            let hi_rpm = rpm.max(FAN_SET_RPM_HIGHTEMP);
            self.ec.write_byte(0x0257, hi_rpm)?;
            self.ec.write_byte(0x0258, hi_rpm)?;
        }

        Ok(())
    }

    /// Read fan 2 manual enable state.
    pub fn read_fan2_enable(&self) -> io::Result<bool> {
        if self.num_fans < 2 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no fan 2"));
        }
        Ok(self.ec.read_byte(0x0240)? & 0x01 != 0)
    }

    /// Set fan 2 manual/auto mode.
    pub fn write_fan2_enable(&self, enable: bool) -> io::Result<()> {
        if self.num_fans < 2 {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no fan 2"));
        }
        self.ec.write_byte(0x0240, if enable { 0x01 } else { 0x00 })
    }

    // --- Generic fan index API ---

    /// Set fan speed by index (0-based). Enables manual mode first.
    pub fn set_fan_pwm(&self, index: u8, pwm: u8) -> io::Result<()> {
        match index {
            0 => {
                self.write_fan1_enable(true)?;
                self.write_fan1_pwm(pwm)
            }
            1 => {
                self.write_fan2_enable(true)?;
                self.write_fan2_pwm(pwm)
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fan index")),
        }
    }

    /// Set fan to auto mode by index (0-based).
    pub fn set_fan_auto(&self, index: u8) -> io::Result<()> {
        match index {
            0 => self.write_fan1_enable(false),
            1 => self.write_fan2_enable(false),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fan index")),
        }
    }

    /// Read the current PWM for fan by index (0-based).
    pub fn get_fan_pwm(&self, index: u8) -> io::Result<u8> {
        match index {
            0 => self.read_fan1_pwm(),
            1 => self.read_fan2_pwm(),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fan index")),
        }
    }

    /// Restore all fans to auto mode (for graceful shutdown).
    pub fn restore_auto(&self) -> io::Result<()> {
        self.write_fan1_enable(false)?;
        if self.num_fans >= 2 {
            self.write_fan2_enable(false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deadband_off() {
        // 12.5% of 0xb8 (184) = 23 → values ≤23 snap to 0
        assert_eq!(apply_deadband(0, FAN_SET_DUTY_MAX), 0);
        assert_eq!(apply_deadband(23, FAN_SET_DUTY_MAX), 0);
    }

    #[test]
    fn test_deadband_snap_up() {
        // 25% of 184 = 46 → values 24..45 snap to 46
        assert_eq!(apply_deadband(24, FAN_SET_DUTY_MAX), 46);
        assert_eq!(apply_deadband(45, FAN_SET_DUTY_MAX), 46);
    }

    #[test]
    fn test_deadband_pass_through() {
        assert_eq!(apply_deadband(46, FAN_SET_DUTY_MAX), 46);
        assert_eq!(apply_deadband(184, FAN_SET_DUTY_MAX), 184);
    }

    #[test]
    fn test_pwm_duty_conversion() {
        assert_eq!(pwm_to_duty(0), 0);
        assert_eq!(pwm_to_duty(255), FAN_SET_DUTY_MAX);
        assert_eq!(duty_to_pwm(0), 0);
        assert_eq!(duty_to_pwm(FAN_SET_DUTY_MAX as u8), 255);
    }

    #[test]
    fn test_pwm_rpm_conversion() {
        assert_eq!(pwm_to_rpm(0), 0);
        assert_eq!(pwm_to_rpm(255), FAN_SET_RPM_MAX);
    }
}
