pub mod hidraw;
pub mod ite8291;
pub mod ite8291_lb;
pub mod ite8297;
pub mod ite829x;
pub mod discover;
pub mod color_scaling;

/// RGB color value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const WHITE: Rgb = Rgb::new(0xff, 0xff, 0xff);
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
}

/// Common keyboard LED device trait.
pub trait KeyboardLed: Send {
    /// Set global brightness (0-255 normalized, implementation clamps to device range).
    fn set_brightness(&mut self, brightness: u8) -> std::io::Result<()>;

    /// Get the device-specific maximum brightness.
    fn max_brightness(&self) -> u8;

    /// Set a single zone/key color.
    fn set_color(&mut self, zone: u32, color: Rgb) -> std::io::Result<()>;

    /// Get number of controllable zones (1 for lightbar, 4 for zones, 126 for per-key).
    fn zone_count(&self) -> u32;

    /// Turn LEDs off.
    fn turn_off(&mut self) -> std::io::Result<()>;

    /// Turn LEDs on (restore last state).
    fn turn_on(&mut self) -> std::io::Result<()>;

    /// Flush pending changes to hardware (for batched updates).
    fn flush(&mut self) -> std::io::Result<()>;

    /// Device type identifier.
    fn device_type(&self) -> &str;

    /// Available animation modes.
    fn available_modes(&self) -> Vec<&str>;
}
