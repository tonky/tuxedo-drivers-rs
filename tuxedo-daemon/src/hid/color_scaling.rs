//! Per-model RGB color correction for ITE keyboards.
//!
//! Ported from `color_scaling()` in ite_8291.c and ite_8291_lb.c.
//! Without these corrections, colors appear visibly wrong on every model.

use super::Rgb;

/// Color scaling configuration for a specific device model.
#[derive(Debug, Clone)]
pub struct ColorScaling {
    product_sku: String,
    board_name: String,
}

impl ColorScaling {
    /// Create from DMI fields.
    pub fn new(product_sku: &str, board_name: &str) -> Self {
        Self {
            product_sku: product_sku.to_string(),
            board_name: board_name.to_string(),
        }
    }

    /// Apply model-specific color correction.
    ///
    /// `product_id` and `bcd_device` come from the USB HID descriptor.
    /// `row` and `col` are provided for per-key devices (per-row corrections).
    pub fn apply(
        &self,
        color: Rgb,
        product_id: u16,
        bcd_device: u16,
        row: Option<u8>,
        _col: Option<u8>,
    ) -> Rgb {
        let mut r = color.r;
        let mut g = color.g;
        let mut b = color.b;
        let sku = self.product_sku.as_str();
        let board = self.board_name.as_str();

        if sku == "STEPOL1XA04" && product_id == 0x600a {
            r = scale(r, 126);
        } else if sku == "STELLARIS1XI05" && product_id == 0x600a {
            r = scale(r, 200);
            b = scale(b, 220);
            if row == Some(5) {
                r = scale(r, 230);
                b = scale(b, 200);
            }
        } else if sku == "STELLARIS1XA05" {
            r = scale(r, 128);
        } else if sku == "STELLARIS1XI05" && product_id == 0xce00 && bcd_device == 0x0002 {
            // red stays at 100%
            g = scale(g, 220);
            b = scale(b, 200);
        } else if sku == "STELLARIS17I06" && product_id == 0xce00 && bcd_device == 0x0002 {
            g = scale(g, 180);
            b = scale(b, 180);
        } else if sku == "STELLARIS17I06" && product_id == 0x600a {
            r = scale(r, 200);
            b = scale(b, 220);
        } else if (sku == "STELLSL15I06" || sku == "STELLARIS16I06") && product_id == 0x600b {
            r = scale(r, 155);
            b = scale(b, 140);
            if row == Some(0) {
                // Bottom row: amplify
                r = scale(r, 279);
                b = scale(b, 282);
            }
            if row == Some(5) {
                // Top row: reduce
                r = scale(r, 148);
                b = scale(b, 137);
            }
        } else if (sku == "STELLARIS16I07" || sku == "STELLARIS16A07" || board == "X6AR55xU")
            && product_id == 0x600b
        {
            r = scale(r, 170);
            b = scale(b, 125);
        } else if board == "X5KK45xS_X5SP45xS" {
            r = scale(r, 180);
        } else if board == "X5AR45xS" {
            // No scaling
        } else {
            // Default scaling for all other models
            g = scale(g, 126);
            b = scale(b, 120);
        }

        Rgb::new(r, g, b)
    }

    /// Apply lightbar-specific color scaling (ite_8291_lb).
    pub fn apply_lightbar(&self, color: Rgb, product_id: u16) -> Rgb {
        let r = color.r;
        let mut g = color.g;
        let mut b = color.b;
        let sku = self.product_sku.as_str();

        if (sku == "STEPOL1XA04" || sku == "STELLARIS1XI05" || sku == "STELLARIS17I06")
            && product_id == 0x6010
        {
            g = scale(g, 100);
            b = scale(b, 100);
        }

        Rgb::new(r, g, b)
    }
}

/// Scale a color channel: `(factor * value) / 255`.
/// Factor > 255 amplifies (used for bottom-row correction on some models).
fn scale(value: u8, factor: u16) -> u8 {
    ((factor as u32 * value as u32) / 255).min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_scaling() {
        let cs = ColorScaling::new("UNKNOWN_SKU", "UNKNOWN_BOARD");
        let result = cs.apply(Rgb::WHITE, 0x6004, 0, None, None);
        // Default: green ~49%, blue ~47%
        assert_eq!(result.r, 255);
        assert_eq!(result.g, 126); // (126 * 255) / 255
        assert_eq!(result.b, 120); // (120 * 255) / 255
    }

    #[test]
    fn test_no_scaling_x5ar() {
        let cs = ColorScaling::new("", "X5AR45xS");
        let result = cs.apply(Rgb::WHITE, 0x6004, 0, None, None);
        assert_eq!(result, Rgb::WHITE);
    }

    #[test]
    fn test_stepol_scaling() {
        let cs = ColorScaling::new("STEPOL1XA04", "");
        let result = cs.apply(Rgb::WHITE, 0x600a, 0, None, None);
        assert_eq!(result.r, 126); // red scaled to 49%
        assert_eq!(result.g, 255); // green unchanged
        assert_eq!(result.b, 255); // blue unchanged
    }

    #[test]
    fn test_stellsl_row_correction() {
        let cs = ColorScaling::new("STELLSL15I06", "");
        // Bottom row (row 0): base scaling then amplify
        let result = cs.apply(Rgb::WHITE, 0x600b, 0, Some(0), None);
        // r: scale(255, 155) = 155, then scale(155, 279) = 169
        let r_base = scale(255, 155);
        let r_bottom = scale(r_base, 279);
        assert_eq!(result.r, r_bottom);
    }

    #[test]
    fn test_scale_function() {
        assert_eq!(scale(255, 255), 255); // 100%
        assert_eq!(scale(255, 128), 128); // ~50%
        assert_eq!(scale(255, 0), 0);     // 0%
        assert_eq!(scale(0, 255), 0);     // 0 input
        // Amplification (factor > 255) clamps to 255
        assert_eq!(scale(255, 300), 255);
    }

    #[test]
    fn test_lightbar_scaling() {
        let cs = ColorScaling::new("STEPOL1XA04", "");
        let result = cs.apply_lightbar(Rgb::WHITE, 0x6010);
        assert_eq!(result.r, 255);
        assert_eq!(result.g, 100);
        assert_eq!(result.b, 100);
    }
}
