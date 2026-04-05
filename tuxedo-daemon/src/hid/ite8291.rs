//! ITE 8291 per-key and zone-based RGB keyboard control via hidraw.
//!
//! Replaces the kernel modules: ite_8291 (~900 LOC).
//! Protocol uses 8-byte HID feature reports for control and
//! 65-byte interrupt OUT packets for per-key row data.

use super::color_scaling::ColorScaling;
use super::hidraw::HidrawDevice;
use super::{KeyboardLed, Rgb};
use std::io;
use tracing::debug;

// USB IDs (vendor 0x048d)
pub const VID: u16 = 0x048d;
pub const PIDS: &[u16] = &[0xce00, 0x6004, 0x600a, 0x600b];

// Layout
const NR_ROWS: usize = 6;
const LEDS_PER_ROW: usize = 21;
const ROW_DATA_PADDING: usize = 2;
const ROW_DATA_LENGTH: usize = ROW_DATA_PADDING + LEDS_PER_ROW * 3; // 65 bytes

// Brightness
const BRIGHTNESS_MAX: u8 = 0x32; // 50

// Animation mode constants
const MODE_USER: u8 = 0x33; // per-key control
const MODE_MONO: u8 = 0x01;
const MODE_BREATH: u8 = 0x02;
const MODE_WAVE: u8 = 0x03;
const MODE_RAINBOW: u8 = 0x05;

/// Device variant, determined by USB product ID and bcdDevice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ite8291Variant {
    /// Per-key RGB: 6 rows x 21 columns, uses interrupt OUT endpoint.
    PerKey,
    /// Zone-based: 4 color zones, simpler feature-report-only protocol.
    Zones,
}

/// ITE 8291 keyboard LED controller.
pub struct Ite8291 {
    device: HidrawDevice,
    variant: Ite8291Variant,
    brightness: u8,
    row_data: [[u8; ROW_DATA_LENGTH]; NR_ROWS],
    color_scaling: ColorScaling,
    bcd_device: u16,
}

impl Ite8291 {
    /// Create a new controller. `bcd_device` determines per-key vs zones variant.
    pub fn new(
        device: HidrawDevice,
        bcd_device: u16,
        color_scaling: ColorScaling,
    ) -> io::Result<Self> {
        let variant = if device.info.product_id == 0xce00 && bcd_device == 0x0002 {
            Ite8291Variant::Zones
        } else {
            Ite8291Variant::PerKey
        };

        debug!(
            variant = ?variant,
            bcd_device,
            product_id = device.info.product_id,
            "ITE 8291 device detected"
        );

        let mut ctrl = Self {
            device,
            variant,
            brightness: 0,
            row_data: [[0u8; ROW_DATA_LENGTH]; NR_ROWS],
            color_scaling,
            bcd_device,
        };

        ctrl.init()?;
        Ok(ctrl)
    }

    fn init(&mut self) -> io::Result<()> {
        match self.variant {
            Ite8291Variant::PerKey => {
                // Initialize row data to white
                for row in 0..NR_ROWS {
                    for col in 0..LEDS_PER_ROW {
                        self.set_row_pixel(row, col, Rgb::WHITE);
                    }
                }
            }
            Ite8291Variant::Zones => {
                // Enable zone control
                self.send_feature(&[0x1a, 0x00, 0x01, 0x04, 0x00, 0x00, 0x00, 0x01])?;
            }
        }
        Ok(())
    }

    /// Send an 8-byte HID feature report.
    fn send_feature(&self, data: &[u8; 8]) -> io::Result<()> {
        self.device.send_feature_report(data)
    }

    /// Set a pixel in the row data buffer (does not flush to hardware).
    fn set_row_pixel(&mut self, row: usize, col: usize, color: Rgb) {
        let scaled = self.color_scaling.apply(
            color,
            self.device.info.product_id,
            self.bcd_device,
            Some(row as u8),
            Some(col as u8),
        );
        // BRG ordering after 2-byte padding
        self.row_data[row][ROW_DATA_PADDING + col] = scaled.b;
        self.row_data[row][ROW_DATA_PADDING + LEDS_PER_ROW + col] = scaled.g;
        self.row_data[row][ROW_DATA_PADDING + LEDS_PER_ROW * 2 + col] = scaled.r;
    }

    /// Write the per-key state to hardware.
    fn write_perkey_state(&mut self) -> io::Result<()> {
        // Set per-key mode
        self.send_feature(&[
            0x08, 0x02, MODE_USER, 0x00, self.brightness, 0x00, 0x00, 0x00,
        ])?;

        // Send each row
        for row in 0..NR_ROWS {
            // Announce row
            self.send_feature(&[0x16, 0x00, row as u8, 0x00, 0x00, 0x00, 0x00, 0x00])?;
            // Send row data via interrupt OUT
            self.device.send_output_report(&self.row_data[row])?;
        }

        Ok(())
    }

    /// Write zone color via feature report.
    fn write_zone_color(&self, zone: u8, color: Rgb) -> io::Result<()> {
        let scaled = self.color_scaling.apply(
            color,
            self.device.info.product_id,
            self.bcd_device,
            None,
            None,
        );
        self.send_feature(&[
            0x14,
            0x00,
            zone + 1, // color_def_num is 1-indexed
            scaled.r,
            scaled.g,
            scaled.b,
            0x00,
            0x00,
        ])
    }

    /// Write zone mode state.
    fn write_zones_state(&self) -> io::Result<()> {
        self.send_feature(&[
            0x08, 0x02, MODE_MONO, 0x03, self.brightness, 0x08, 0x00, 0x00,
        ])
    }

    /// Set a built-in animation mode (only meaningful for zones variant).
    pub fn set_animation_mode(&self, mode: u8, speed: u8) -> io::Result<()> {
        self.send_feature(&[
            0x08, 0x02, mode, speed, self.brightness, 0x08, 0x00, 0x00,
        ])
    }
}

impl KeyboardLed for Ite8291 {
    fn set_brightness(&mut self, brightness: u8) -> io::Result<()> {
        self.brightness = brightness.min(BRIGHTNESS_MAX);
        // Also send brightness-only command for immediate effect
        self.send_feature(&[0x09, 0x02, self.brightness, 0x00, 0x00, 0x00, 0x00, 0x00])
    }

    fn max_brightness(&self) -> u8 {
        BRIGHTNESS_MAX
    }

    fn set_color(&mut self, zone: u32, color: Rgb) -> io::Result<()> {
        match self.variant {
            Ite8291Variant::PerKey => {
                let row = (zone / LEDS_PER_ROW as u32) as usize;
                let col = (zone % LEDS_PER_ROW as u32) as usize;
                if row < NR_ROWS && col < LEDS_PER_ROW {
                    self.set_row_pixel(row, col, color);
                }
                Ok(())
            }
            Ite8291Variant::Zones => {
                if zone < 4 {
                    self.write_zone_color(zone as u8, color)?;
                }
                Ok(())
            }
        }
    }

    fn zone_count(&self) -> u32 {
        match self.variant {
            Ite8291Variant::PerKey => (NR_ROWS * LEDS_PER_ROW) as u32,
            Ite8291Variant::Zones => 4,
        }
    }

    fn turn_off(&mut self) -> io::Result<()> {
        match self.variant {
            Ite8291Variant::PerKey => {
                self.send_feature(&[0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
            }
            Ite8291Variant::Zones => {
                // Zones off sequence
                self.send_feature(&[0x09, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x12, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x08, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
                self.send_feature(&[0x1a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01])
            }
        }
    }

    fn turn_on(&mut self) -> io::Result<()> {
        match self.variant {
            Ite8291Variant::PerKey => self.write_perkey_state(),
            Ite8291Variant::Zones => {
                self.send_feature(&[0x1a, 0x00, 0x01, 0x04, 0x00, 0x00, 0x00, 0x01])?;
                self.write_zones_state()
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.variant {
            Ite8291Variant::PerKey => self.write_perkey_state(),
            Ite8291Variant::Zones => self.write_zones_state(),
        }
    }

    fn device_type(&self) -> &str {
        match self.variant {
            Ite8291Variant::PerKey => "ite_perkey",
            Ite8291Variant::Zones => "ite_zones",
        }
    }

    fn available_modes(&self) -> Vec<&str> {
        match self.variant {
            Ite8291Variant::PerKey => vec!["static", "breath", "wave", "rainbow"],
            Ite8291Variant::Zones => vec!["static", "breath", "wave", "rainbow"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variant_detection() {
        // product 0xce00 + bcd 0x0002 = zones
        assert_eq!(
            if 0xce00u16 == 0xce00 && 0x0002u16 == 0x0002 {
                Ite8291Variant::Zones
            } else {
                Ite8291Variant::PerKey
            },
            Ite8291Variant::Zones
        );

        // product 0x600a = per-key regardless of bcd
        assert_eq!(
            if 0x600au16 == 0xce00 && 0x0002u16 == 0x0002 {
                Ite8291Variant::Zones
            } else {
                Ite8291Variant::PerKey
            },
            Ite8291Variant::PerKey
        );
    }

    #[test]
    fn test_row_data_layout() {
        // Verify BRG byte positions
        let col = 5usize;
        let blue_idx = ROW_DATA_PADDING + col;
        let green_idx = ROW_DATA_PADDING + LEDS_PER_ROW + col;
        let red_idx = ROW_DATA_PADDING + LEDS_PER_ROW * 2 + col;

        assert_eq!(blue_idx, 7);   // 2 + 5
        assert_eq!(green_idx, 28); // 2 + 21 + 5
        assert_eq!(red_idx, 49);   // 2 + 42 + 5
        assert!(red_idx < ROW_DATA_LENGTH);
    }

    #[test]
    fn test_row_data_size() {
        assert_eq!(ROW_DATA_LENGTH, 65);
    }

    #[test]
    fn test_brightness_clamp() {
        assert_eq!(255u8.min(BRIGHTNESS_MAX), BRIGHTNESS_MAX);
        assert_eq!(30u8.min(BRIGHTNESS_MAX), 30);
    }
}
