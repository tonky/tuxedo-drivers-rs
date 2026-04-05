use crate::config::Config;
use crate::dmi::TuxedoDevice;
use crate::fan_curve::FanBackend;
use crate::hid::color_scaling::ColorScaling;
use crate::hid::discover;
use crate::hid::{KeyboardLed, Rgb};
use crate::nb04::Nb04Platform;
use crate::nb05::Nb05Platform;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use zbus::connection::Builder;
use zbus::fdo;
use zbus::object_server::SignalEmitter;

const BUS_NAME: &str = "com.tuxedo.Daemon";

/// Start the D-Bus service and return the connection. Does not block.
///
/// `fan_backend` is used for fan/temperature control (works with any platform).
/// `nb05` is used for NB05-specific features (keyboard backlight via EC).
/// `nb04` is used for NB04-specific features (power profiles, sensors).
pub async fn serve(
    device: Option<TuxedoDevice>,
    config: Config,
    fan_backend: Option<Arc<dyn FanBackend>>,
    nb05: Option<Arc<Nb05Platform>>,
    nb04: Option<Arc<Nb04Platform>>,
) -> anyhow::Result<zbus::Connection> {
    let fan = FanInterface::new(&config, fan_backend);
    let keyboard = KeyboardInterface::new(&device, nb05.clone());
    let profile = ProfileInterface::new(&config, nb04);
    let device_iface = DeviceInterface::new(device);

    let conn = Builder::system()?
        .name(BUS_NAME)?
        .serve_at("/com/tuxedo/Daemon", fan)?
        .serve_at("/com/tuxedo/Daemon", keyboard)?
        .serve_at("/com/tuxedo/Daemon", profile)?
        .serve_at("/com/tuxedo/Daemon", device_iface)?
        .build()
        .await?;

    info!(bus_name = BUS_NAME, "D-Bus service registered");

    Ok(conn)
}

// ---------------------------------------------------------------------------
// com.tuxedo.Daemon.Fan
// ---------------------------------------------------------------------------

struct FanInterface {
    backend: Option<Arc<dyn FanBackend>>,
    fan_count: u32,
}

impl FanInterface {
    fn new(_config: &Config, backend: Option<Arc<dyn FanBackend>>) -> Self {
        let fan_count = backend.as_ref().map_or(0, |b| b.num_fans() as u32);
        Self { backend, fan_count }
    }

    fn require_backend(&self) -> fdo::Result<&dyn FanBackend> {
        self.backend
            .as_deref()
            .ok_or_else(|| fdo::Error::NotSupported("no fan backend available".into()))
    }
}

#[zbus::interface(name = "com.tuxedo.Daemon.Fan")]
impl FanInterface {
    async fn set_fan_speed(&self, fan_index: u32, pwm: u8) -> fdo::Result<()> {
        let backend = self.require_backend()?;
        backend
            .write_pwm(fan_index as u8, pwm)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn set_auto_mode(&self, fan_index: u32) -> fdo::Result<()> {
        let backend = self.require_backend()?;
        backend
            .set_auto(fan_index as u8)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn get_fan_speed(&self, fan_index: u32) -> fdo::Result<u32> {
        let backend = self.require_backend()?;
        backend
            .read_fan_rpm(fan_index as u8)
            .map(|r| r as u32)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn get_temperature(&self, sensor_index: u32) -> fdo::Result<i32> {
        let backend = self.require_backend()?;
        if sensor_index != 0 {
            return Err(fdo::Error::InvalidArgs("only sensor 0 (CPU) supported".into()));
        }
        backend
            .read_temp()
            .map(|t| t as i32 * 1000) // millidegrees
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn get_fan_info(&self) -> fdo::Result<(u32, u32, bool, u8)> {
        let backend = self.require_backend()?;
        let num_fans = backend.num_fans();
        // Generic defaults — platform-specific limits can be refined later
        Ok((
            5400, // max RPM (generic estimate)
            0,    // min RPM
            num_fans > 1,
            num_fans,
        ))
    }

    #[zbus(property)]
    async fn fan_count(&self) -> u32 {
        self.fan_count
    }

    #[zbus(signal)]
    async fn fan_speed_changed(
        emitter: &SignalEmitter<'_>,
        fan_index: u32,
        rpm: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn temperature_changed(
        emitter: &SignalEmitter<'_>,
        sensor_index: u32,
        millidegrees: i32,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// com.tuxedo.Daemon.Keyboard
// ---------------------------------------------------------------------------

struct KeyboardInterface {
    led: Mutex<Option<Box<dyn KeyboardLed>>>,
    nb05: Option<Arc<Nb05Platform>>,
}

impl KeyboardInterface {
    fn new(device: &Option<TuxedoDevice>, nb05: Option<Arc<Nb05Platform>>) -> Self {
        let product_sku = device
            .as_ref()
            .map(|d| d.product_sku.as_str())
            .unwrap_or("");
        let board_name = crate::dmi::read_dmi_field("board_name").unwrap_or_default();
        let color_scaling = ColorScaling::new(product_sku, &board_name);

        // Try to find and open an ITE HID keyboard LED device
        let discovered = discover::scan();
        let led = if let Some(disc) = discovered.into_iter().next() {
            match discover::open_device(&disc, color_scaling) {
                Ok(led) => {
                    info!(device_type = led.device_type(), "opened ITE LED device");
                    Some(led)
                }
                Err(e) => {
                    warn!("failed to open ITE LED device: {e}");
                    None
                }
            }
        } else {
            info!("no ITE LED device found");
            None
        };

        Self {
            led: Mutex::new(led),
            nb05,
        }
    }
}

#[zbus::interface(name = "com.tuxedo.Daemon.Keyboard")]
impl KeyboardInterface {
    async fn set_brightness(&self, brightness: u8) -> fdo::Result<()> {
        // Try ITE HID first
        let mut guard = self.led.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(led) = guard.as_mut() {
            return led
                .set_brightness(brightness)
                .map_err(|e| fdo::Error::Failed(e.to_string()));
        }
        drop(guard);

        // Fall back to NB05 EC backlight
        if let Some(nb05) = &self.nb05 {
            return nb05
                .kbd_bl
                .set_brightness(nb05.fan_ctl.ec(), brightness)
                .map_err(|e| fdo::Error::Failed(e.to_string()));
        }

        Err(fdo::Error::NotSupported("no keyboard LED device".into()))
    }

    async fn set_color(&self, zone: u32, r: u8, g: u8, b: u8) -> fdo::Result<()> {
        let mut guard = self.led.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(led) => led
                .set_color(zone, Rgb::new(r, g, b))
                .map_err(|e| fdo::Error::Failed(e.to_string())),
            None => Err(fdo::Error::NotSupported("no keyboard LED device".into())),
        }
    }

    async fn set_mode(&self, mode: &str) -> fdo::Result<()> {
        let _ = mode;
        // Animation mode switching will be implemented with the animation engine
        Err(fdo::Error::NotSupported(
            "animation modes not yet implemented".into(),
        ))
    }

    async fn get_keyboard_info(&self) -> fdo::Result<(String, u8, u32, Vec<String>)> {
        let guard = self.led.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(led) = guard.as_ref() {
            return Ok((
                led.device_type().to_string(),
                led.max_brightness(),
                led.zone_count(),
                led.available_modes().into_iter().map(String::from).collect(),
            ));
        }
        drop(guard);

        // NB05 EC backlight info
        if self.nb05.is_some() {
            return Ok((
                "nb05-ec-white".to_string(),
                crate::nb05::kbd_backlight::BRIGHTNESS_MAX,
                1,
                vec!["static".to_string()],
            ));
        }

        Err(fdo::Error::NotSupported("no keyboard LED device".into()))
    }

    #[zbus(signal)]
    async fn brightness_changed(
        emitter: &SignalEmitter<'_>,
        brightness: u8,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// com.tuxedo.Daemon.Profile
// ---------------------------------------------------------------------------

struct ProfileInterface {
    current_profile: Mutex<String>,
    available_profiles: Vec<String>,
    nb04: Option<Arc<Nb04Platform>>,
}

impl ProfileInterface {
    fn new(config: &Config, nb04: Option<Arc<Nb04Platform>>) -> Self {
        Self {
            current_profile: Mutex::new(config.profile.default.clone()),
            available_profiles: vec![
                "powersave".into(),
                "balanced".into(),
                "performance".into(),
            ],
            nb04,
        }
    }
}

#[zbus::interface(name = "com.tuxedo.Daemon.Profile")]
impl ProfileInterface {
    async fn set_profile(&self, profile: &str) -> fdo::Result<()> {
        if let Some(nb04) = &self.nb04 {
            let pp = crate::nb04::PowerProfile::from_str(profile).ok_or_else(|| {
                fdo::Error::InvalidArgs(format!(
                    "unknown profile '{}', use: battery, balanced, performance",
                    profile
                ))
            })?;
            nb04.set_profile(pp)
                .map_err(|e| fdo::Error::Failed(e.to_string()))?;
            if let Ok(mut cached) = self.current_profile.lock() {
                *cached = profile.to_string();
            }
            Ok(())
        } else {
            Err(fdo::Error::NotSupported(
                "power profiles not supported on this platform".into(),
            ))
        }
    }

    #[zbus(property)]
    async fn current_profile(&self) -> String {
        self.current_profile
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "unknown".into())
    }

    #[zbus(property)]
    async fn available_profiles(&self) -> Vec<String> {
        self.available_profiles.clone()
    }

    #[zbus(signal)]
    async fn profile_changed(
        emitter: &SignalEmitter<'_>,
        profile: &str,
    ) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// com.tuxedo.Daemon.Device
// ---------------------------------------------------------------------------

struct DeviceInterface {
    device_name: String,
    platform: String,
}

impl DeviceInterface {
    fn new(device: Option<TuxedoDevice>) -> Self {
        match device {
            Some(dev) => Self {
                device_name: dev.name.to_string(),
                platform: dev.platform.as_str().to_string(),
            },
            None => Self {
                device_name: "Unknown".into(),
                platform: "Unknown".into(),
            },
        }
    }
}

#[zbus::interface(name = "com.tuxedo.Daemon.Device")]
impl DeviceInterface {
    #[zbus(property)]
    async fn device_name(&self) -> &str {
        &self.device_name
    }

    #[zbus(property)]
    async fn platform(&self) -> &str {
        &self.platform
    }

    #[zbus(property)]
    async fn daemon_version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    #[zbus(signal)]
    async fn device_hotplug(
        emitter: &SignalEmitter<'_>,
        subsystem: &str,
        action: &str,
    ) -> zbus::Result<()>;
}
