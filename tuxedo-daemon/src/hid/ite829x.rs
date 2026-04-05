//! ITE 829x per-key RGB keyboard control via hidraw.
//!
//! Replaces the kernel module: ite_829x.
//! Simple protocol: 6-byte feature reports with 0xcc prefix.
//! 6 rows x 20 columns, 120 individual RGB LEDs.

use super::hidraw::HidrawDevice;
use super::{KeyboardLed, Rgb};
use std::io;

// USB IDs
pub const VID: u16 = 0x048d;
pub const PID: u16 = 0x8910;

const HID_DATA_SIZE: usize = 6;
const BRIGHTNESS_MAX: u8 = 0x0a; // 10 levels
const ROWS: usize = 6;
const COLS: usize = 20;

/// ITE 829x keyboard controller.
pub struct Ite829x {
    device: HidrawDevice,
    brightness: u8,
    /// Per-key color buffer.
    keys: [[Rgb; COLS]; ROWS],
}

impl Ite829x {
    pub fn new(device: HidrawDevice) -> io::Result<Self> {
        let mut ctrl = Self {
            device,
            brightness: 0,
            keys: [[Rgb::WHITE; COLS]; ROWS],
        };
        ctrl.init()?;
        Ok(ctrl)
    }

    fn init(&mut self) -> io::Result<()> {
        // Set brightness to 0 initially
        self.send_cmd(0x09, 0x00, 0x02, 0x00, 0x00)?;
        // Initialize all keys to white
        for row in 0..ROWS {
            for col in 0..COLS {
                self.write_key(row, col, Rgb::WHITE)?;
            }
        }
        Ok(())
    }

    fn send_cmd(&self, cmd: u8, d0: u8, d1: u8, d2: u8, d3: u8) -> io::Result<()> {
        let buf = [0xcc, cmd, d0, d1, d2, d3];
        self.device.send_feature_report(&buf)
    }

    /// LED ID encoding: `((row & 0x07) << 5) | (col & 0x1f)`
    fn led_id(row: usize, col: usize) -> u8 {
        ((row as u8 & 0x07) << 5) | (col as u8 & 0x1f)
    }

    fn write_key(&self, row: usize, col: usize, color: Rgb) -> io::Result<()> {
        self.send_cmd(0x01, Self::led_id(row, col), color.r, color.g, color.b)
    }
}

impl KeyboardLed for Ite829x {
    fn set_brightness(&mut self, brightness: u8) -> io::Result<()> {
        self.brightness = brightness.min(BRIGHTNESS_MAX);
        self.send_cmd(0x09, self.brightness, 0x02, 0x00, 0x00)
    }

    fn max_brightness(&self) -> u8 {
        BRIGHTNESS_MAX
    }

    fn set_color(&mut self, zone: u32, color: Rgb) -> io::Result<()> {
        let row = (zone / COLS as u32) as usize;
        let col = (zone % COLS as u32) as usize;
        if row < ROWS && col < COLS {
            self.keys[row][col] = color;
            self.write_key(row, col, color)?;
        }
        Ok(())
    }

    fn zone_count(&self) -> u32 {
        (ROWS * COLS) as u32
    }

    fn turn_off(&mut self) -> io::Result<()> {
        self.send_cmd(0x09, 0x00, 0x02, 0x00, 0x00)
    }

    fn turn_on(&mut self) -> io::Result<()> {
        self.send_cmd(0x09, self.brightness, 0x02, 0x00, 0x00)
    }

    fn flush(&mut self) -> io::Result<()> {
        for row in 0..ROWS {
            for col in 0..COLS {
                self.write_key(row, col, self.keys[row][col])?;
            }
        }
        Ok(())
    }

    fn device_type(&self) -> &str {
        "ite_829x"
    }

    fn available_modes(&self) -> Vec<&str> {
        vec!["static"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_led_id_encoding() {
        assert_eq!(Ite829x::led_id(0, 0), 0x00);
        assert_eq!(Ite829x::led_id(0, 19), 0x13);
        assert_eq!(Ite829x::led_id(1, 0), 0x20);
        assert_eq!(Ite829x::led_id(5, 19), 0xb3);
    }
}
