//! ITE 8291 lightbar variant control via hidraw.
//!
//! Replaces the kernel module: ite_8291_lb.
//! Three sub-variants (0x6010, 0x7000, 0x7001) with different command bytes.

use super::color_scaling::ColorScaling;
use super::hidraw::HidrawDevice;
use super::{KeyboardLed, Rgb};
use std::io;

// USB IDs (vendor 0x048d)
pub const VID: u16 = 0x048d;
pub const PIDS: &[u16] = &[0x6010, 0x7000, 0x7001];

// Brightness range
const BRIGHTNESS_MAX: u8 = 0x64; // 100
const SPEED_MIN: u8 = 0x01;
const SPEED_MAX: u8 = 0x0a;

/// Sub-variant determined by USB product ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LbVariant {
    /// PID 0x6010: variant byte 0x00, command byte 0x02
    Standard,
    /// PID 0x7000: variant byte 0x01, command byte 0x21
    Enhanced,
    /// PID 0x7001: variant byte 0x00, command byte 0x22
    Variant7001,
}

impl LbVariant {
    fn from_pid(pid: u16) -> Self {
        match pid {
            0x7000 => LbVariant::Enhanced,
            0x7001 => LbVariant::Variant7001,
            _ => LbVariant::Standard,
        }
    }

    fn color_variant_byte(&self) -> u8 {
        match self {
            LbVariant::Standard | LbVariant::Variant7001 => 0x00,
            LbVariant::Enhanced => 0x01,
        }
    }

    fn cmd_variant_byte(&self) -> u8 {
        match self {
            LbVariant::Standard => 0x02,
            LbVariant::Enhanced => 0x21,
            LbVariant::Variant7001 => 0x22,
        }
    }

    fn mode_config_byte(&self) -> u8 {
        match self {
            LbVariant::Standard => 0x08,
            LbVariant::Enhanced | LbVariant::Variant7001 => 0x01,
        }
    }

    fn on_variant_byte(&self) -> u8 {
        match self {
            LbVariant::Standard | LbVariant::Variant7001 => 0x00,
            LbVariant::Enhanced => 0x01,
        }
    }
}

/// ITE 8291 lightbar controller.
pub struct Ite8291Lb {
    device: HidrawDevice,
    variant: LbVariant,
    brightness: u8,
    color: Rgb,
    color_scaling: ColorScaling,
}

impl Ite8291Lb {
    pub fn new(device: HidrawDevice, color_scaling: ColorScaling) -> io::Result<Self> {
        let variant = LbVariant::from_pid(device.info.product_id);

        let ctrl = Self {
            device,
            variant,
            brightness: 0,
            color: Rgb::WHITE,
            color_scaling,
        };

        ctrl.write_on()?;
        ctrl.write_mono()?;

        Ok(ctrl)
    }

    fn send_feature(&self, data: &[u8; 8]) -> io::Result<()> {
        self.device.send_feature_report(data)
    }

    fn write_on(&self) -> io::Result<()> {
        self.send_feature(&[
            0x1a,
            self.variant.on_variant_byte(),
            0x01,
            0x04,
            0x00,
            0x00,
            0x00,
            0x01,
        ])
    }

    fn write_off(&self) -> io::Result<()> {
        match self.variant {
            LbVariant::Standard | LbVariant::Variant7001 => {
                self.send_feature(&[0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x08, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x1a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01])
            }
            LbVariant::Enhanced => {
                self.send_feature(&[0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x08, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x1a, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01])
            }
        }
    }

    fn write_color(&self, color: Rgb) -> io::Result<()> {
        let scaled = self
            .color_scaling
            .apply_lightbar(color, self.device.info.product_id);
        self.send_feature(&[
            0x14,
            self.variant.color_variant_byte(),
            0x01,
            scaled.r,
            scaled.g,
            scaled.b,
            0x00,
            0x00,
        ])
    }

    fn write_mono(&self) -> io::Result<()> {
        self.write_color(self.color)?;
        self.send_feature(&[
            0x08,
            self.variant.cmd_variant_byte(),
            0x01, // mono mode
            0x01,
            self.brightness,
            self.variant.mode_config_byte(),
            0x00,
            0x00,
        ])
    }

    /// Set breathe animation mode.
    pub fn set_breathe(&self, speed: u8) -> io::Result<()> {
        let speed = speed.clamp(SPEED_MIN, SPEED_MAX);
        self.send_feature(&[
            0x08,
            self.variant.cmd_variant_byte(),
            0x02, // breathe mode
            speed,
            self.brightness,
            self.variant.mode_config_byte(),
            0x00,
            0x00,
        ])
    }
}

impl KeyboardLed for Ite8291Lb {
    fn set_brightness(&mut self, brightness: u8) -> io::Result<()> {
        self.brightness = brightness.min(BRIGHTNESS_MAX);
        self.write_mono()
    }

    fn max_brightness(&self) -> u8 {
        BRIGHTNESS_MAX
    }

    fn set_color(&mut self, _zone: u32, color: Rgb) -> io::Result<()> {
        self.color = color;
        self.write_color(color)?;
        self.write_mono()
    }

    fn zone_count(&self) -> u32 {
        1
    }

    fn turn_off(&mut self) -> io::Result<()> {
        self.write_off()
    }

    fn turn_on(&mut self) -> io::Result<()> {
        self.write_on()?;
        self.write_mono()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.write_mono()
    }

    fn device_type(&self) -> &str {
        "ite_lightbar"
    }

    fn available_modes(&self) -> Vec<&str> {
        match self.variant {
            LbVariant::Standard => vec!["static", "breath", "flash"],
            LbVariant::Enhanced => vec!["static", "breath", "wave", "clash", "catch_up"],
            LbVariant::Variant7001 => vec!["static", "breath"],
        }
    }
}
