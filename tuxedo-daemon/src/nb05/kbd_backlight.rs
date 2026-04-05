//! NB05 keyboard backlight — port of `tuxedo_nb05_kbd_backlight.c`.
//!
//! White-only LED with 3 brightness levels mapped to EC duty values.
//! Register address differs between Pulse (0x0409) and InfinityFlex (0x03e2).

use crate::ec::EcRam;
use std::io;

/// White brightness levels → EC duty values.
const BRIGHTNESS_MAP: [u8; 3] = [0x00, 0x5c, 0xb8];

/// Maximum brightness step (0-indexed, so 0..=2).
pub const BRIGHTNESS_MAX: u8 = 2;

/// Keyboard backlight controller.
pub struct KbdBacklight {
    /// EC register to write brightness.
    register: u16,
}

impl KbdBacklight {
    /// Create a backlight controller for the given product SKU.
    pub fn new(product_sku: &str) -> Self {
        let register = match product_sku {
            "IFLX14I01" => 0x03e2,
            _ => 0x0409,
        };
        Self { register }
    }

    /// Set brightness (0 = off, 1 = dim, 2 = bright).
    pub fn set_brightness(&self, ec: &EcRam, step: u8) -> io::Result<()> {
        if step > BRIGHTNESS_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("brightness step {} exceeds max {}", step, BRIGHTNESS_MAX),
            ));
        }
        ec.write_byte(self.register, BRIGHTNESS_MAP[step as usize])
    }
}
