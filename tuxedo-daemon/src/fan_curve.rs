//! Fan curve engine — generic fan control via temperature-indexed curves.
//!
//! Provides:
//! - `FanBackend` trait abstracting temperature reads and fan PWM writes.
//! - `interpolate()` for linear interpolation between curve points.
//! - `FanCurveEngine` polling loop that reads temp and adjusts fans.

use crate::config::{FanConfig, FanCurvePoint, FanMode};
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{debug, info, warn};

/// Trait abstracting temperature reading and fan speed writing.
/// Implemented by `HwmonDevice` (sysfs) and `Nb05Platform` (EC registers).
pub trait FanBackend: Send + Sync {
    /// Read current CPU temperature in degrees Celsius.
    fn read_temp(&self) -> io::Result<u8>;
    /// Write fan PWM value (0–255) for a given fan index (0-based).
    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()>;
    /// Read current fan PWM value (0–255) for a given fan index (0-based).
    fn read_pwm(&self, fan_index: u8) -> io::Result<u8>;
    /// Set fan to hardware-auto mode for a given fan index (0-based).
    fn set_auto(&self, fan_index: u8) -> io::Result<()>;
    /// Number of fans this backend controls.
    fn num_fans(&self) -> u8;
}

/// Given a temperature and a sorted curve, compute the target speed (0–100%).
///
/// - Below the first point's temp → first point's speed.
/// - Above the last point's temp → last point's speed.
/// - Between two points → linear interpolation.
/// - Empty curve → 100% (safety fallback).
pub fn interpolate(curve: &[FanCurvePoint], temp: u8) -> u8 {
    if curve.is_empty() {
        return 100;
    }
    if temp <= curve[0].temp {
        return curve[0].speed;
    }
    let last = &curve[curve.len() - 1];
    if temp >= last.temp {
        return last.speed;
    }
    for window in curve.windows(2) {
        let lo = &window[0];
        let hi = &window[1];
        if temp >= lo.temp && temp <= hi.temp {
            let t_range = (hi.temp - lo.temp) as u16;
            let s_range = hi.speed as i16 - lo.speed as i16;
            let t_offset = (temp - lo.temp) as u16;
            let speed = lo.speed as i16 + (s_range * t_offset as i16) / t_range as i16;
            return speed.clamp(0, 100) as u8;
        }
    }
    100 // unreachable for valid sorted curves
}

/// Convert a 0–100 percentage speed to 0–255 PWM.
fn percent_to_pwm(percent: u8) -> u8 {
    ((percent as u16 * 255) / 100) as u8
}

/// Returns true if temperature has moved enough from `last_temp` to warrant
/// recalculation, preventing fan speed oscillation on small fluctuations.
fn should_update(current_temp: u8, last_temp: u8, hysteresis: u8) -> bool {
    let diff = if current_temp > last_temp {
        current_temp - last_temp
    } else {
        last_temp - current_temp
    };
    diff >= hysteresis
}

/// Fan curve polling engine.
///
/// Reads temperature periodically, interpolates the configured fan curve,
/// and writes the target PWM to all fans via the `FanBackend`.
pub struct FanCurveEngine {
    backend: Arc<dyn FanBackend>,
    config: watch::Receiver<FanConfig>,
}

impl FanCurveEngine {
    pub fn new(backend: Arc<dyn FanBackend>, config: watch::Receiver<FanConfig>) -> Self {
        Self { backend, config }
    }

    /// Run the fan curve control loop. Spawn this inside `tokio::spawn`.
    pub async fn run(self) {
        let poll_interval = Duration::from_secs(2);
        let hysteresis: u8 = 3;

        let mut last_temp: Option<u8> = None;
        let mut last_pwm: Option<u8> = None;
        let mut interval = tokio::time::interval(poll_interval);

        info!("fan curve engine started");

        loop {
            interval.tick().await;

            let config = self.config.borrow().clone();

            match config.mode {
                FanMode::Auto => {
                    if last_pwm.is_some() {
                        for i in 0..self.backend.num_fans() {
                            if let Err(e) = self.backend.set_auto(i) {
                                warn!(fan = i, "failed to restore auto mode: {e}");
                            }
                        }
                        last_pwm = None;
                        last_temp = None;
                        info!("fan control set to auto mode");
                    }
                    continue;
                }
                FanMode::Manual => {
                    last_temp = None;
                    last_pwm = None;
                    continue;
                }
                FanMode::CustomCurve => {}
            }

            let temp = match self.backend.read_temp() {
                Ok(t) => t,
                Err(e) => {
                    warn!("failed to read temperature: {e}");
                    continue;
                }
            };

            if let Some(lt) = last_temp {
                if !should_update(temp, lt, hysteresis) {
                    continue;
                }
            }

            let speed_percent = interpolate(&config.curve, temp);
            let effective_percent = speed_percent.max(config.min_speed_percent);
            let pwm = percent_to_pwm(effective_percent);

            if last_pwm == Some(pwm) {
                last_temp = Some(temp);
                continue;
            }

            debug!(temp, speed_percent, effective_percent, pwm, "fan curve update");

            for i in 0..self.backend.num_fans() {
                if let Err(e) = self.backend.write_pwm(i, pwm) {
                    warn!(fan = i, "failed to write fan PWM: {e}");
                }
            }

            last_temp = Some(temp);
            last_pwm = Some(pwm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FanCurvePoint;

    fn default_curve() -> Vec<FanCurvePoint> {
        vec![
            FanCurvePoint { temp: 40, speed: 0 },
            FanCurvePoint { temp: 60, speed: 30 },
            FanCurvePoint { temp: 80, speed: 80 },
            FanCurvePoint { temp: 90, speed: 100 },
        ]
    }

    #[test]
    fn test_interpolate_below_first() {
        let curve = default_curve();
        assert_eq!(interpolate(&curve, 20), 0);
        assert_eq!(interpolate(&curve, 40), 0);
    }

    #[test]
    fn test_interpolate_above_last() {
        let curve = default_curve();
        assert_eq!(interpolate(&curve, 90), 100);
        assert_eq!(interpolate(&curve, 100), 100);
    }

    #[test]
    fn test_interpolate_midpoint() {
        let curve = default_curve();
        // At 50°C: midpoint between (40,0) and (60,30) → 15%
        assert_eq!(interpolate(&curve, 50), 15);
    }

    #[test]
    fn test_interpolate_at_points() {
        let curve = default_curve();
        assert_eq!(interpolate(&curve, 60), 30);
        assert_eq!(interpolate(&curve, 80), 80);
    }

    #[test]
    fn test_interpolate_empty_curve() {
        assert_eq!(interpolate(&[], 50), 100);
    }

    #[test]
    fn test_percent_to_pwm() {
        assert_eq!(percent_to_pwm(0), 0);
        assert_eq!(percent_to_pwm(100), 255);
        assert_eq!(percent_to_pwm(50), 127);
    }

    #[test]
    fn test_hysteresis_no_update() {
        assert!(!should_update(50, 48, 3)); // 2 < 3
        assert!(!should_update(50, 50, 3)); // 0 < 3
    }

    #[test]
    fn test_hysteresis_update() {
        assert!(should_update(50, 47, 3)); // 3 >= 3
        assert!(should_update(50, 54, 3)); // 4 >= 3 (cooling)
    }
}
