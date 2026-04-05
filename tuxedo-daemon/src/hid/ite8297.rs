//! ITE 8297 simple RGB lightbar control via hidraw.
//!
//! Replaces the kernel module: ite_8297.
//! Simple protocol: 64-byte feature reports with 0xcc/0xb0 command.

use super::hidraw::HidrawDevice;
use super::{KeyboardLed, Rgb};
use std::io;

// USB IDs
pub const VID: u16 = 0x048d;
pub const PID: u16 = 0x8297;

const HID_DATA_SIZE: usize = 64;
const BRIGHTNESS_MAX: u8 = 0xff;

/// ITE 8297 lightbar controller.
pub struct Ite8297 {
    device: HidrawDevice,
    brightness: u8,
    color: Rgb,
}

impl Ite8297 {
    pub fn new(device: HidrawDevice) -> io::Result<Self> {
        let ctrl = Self {
            device,
            brightness: 0,
            color: Rgb::BLACK,
        };
        ctrl.write_state()?;
        Ok(ctrl)
    }

    fn write_state(&self) -> io::Result<()> {
        let mut buf = [0u8; HID_DATA_SIZE];
        buf[0] = 0xcc;
        buf[1] = 0xb0;
        buf[2] = 0x01;
        buf[3] = 0x01;
        // Scale color by brightness
        buf[4] = ((self.color.r as u16 * self.brightness as u16) / 255) as u8;
        buf[5] = ((self.color.g as u16 * self.brightness as u16) / 255) as u8;
        buf[6] = ((self.color.b as u16 * self.brightness as u16) / 255) as u8;
        self.device.send_feature_report(&buf)
    }
}

impl KeyboardLed for Ite8297 {
    fn set_brightness(&mut self, brightness: u8) -> io::Result<()> {
        self.brightness = brightness;
        self.write_state()
    }

    fn max_brightness(&self) -> u8 {
        BRIGHTNESS_MAX
    }

    fn set_color(&mut self, _zone: u32, color: Rgb) -> io::Result<()> {
        self.color = color;
        self.write_state()
    }

    fn zone_count(&self) -> u32 {
        1
    }

    fn turn_off(&mut self) -> io::Result<()> {
        // Send black color with the proper command prefix
        let mut buf = [0u8; HID_DATA_SIZE];
        buf[0] = 0xcc;
        buf[1] = 0xb0;
        buf[2] = 0x01;
        buf[3] = 0x01;
        // RGB bytes stay 0x00 (black)
        self.device.send_feature_report(&buf)
    }

    fn turn_on(&mut self) -> io::Result<()> {
        self.write_state()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write_state()
    }

    fn device_type(&self) -> &str {
        "ite_8297"
    }

    fn available_modes(&self) -> Vec<&str> {
        vec!["static"]
    }
}
