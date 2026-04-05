use crate::config::Config;
use crate::dmi::TuxedoDevice;
use crate::hid::color_scaling::ColorScaling;
use crate::hid::discover;
use crate::hid::{KeyboardLed, Rgb};
use crate::nb05::Nb05Platform;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};
use zbus::connection::Builder;
use zbus::fdo;
use zbus::object_server::SignalEmitter;

const BUS_NAME: &str = "com.tuxedo.Daemon";

/// Start the D-Bus service and return the connection. Does not block.
///
/// If `nb05` is provided, the FanInterface uses it for real hardware control.
pub async fn serve(
    device: Option<TuxedoDevice>,
    config: Config,
    nb05: Option<Arc<Nb05Platform>>,
) -> anyhow::Result<zbus::Connection> {
    let fan = FanInterface::new(&config, nb05.clone());
    let keyboard = KeyboardInterface::new(&device, nb05.clone());
    let profile = ProfileInterface::new(&config);
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
    nb05: Option<Arc<Nb05Platform>>,
    fan_count: u32,
}

impl FanInterface {
    fn new(_config: &Config, nb05: Option<Arc<Nb05Platform>>) -> Self {
        let fan_count = nb05.as_ref().map_or(0, |p| p.fan_ctl.num_fans() as u32);
        Self { nb05, fan_count }
    }

    fn require_nb05(&self) -> fdo::Result<&Nb05Platform> {
        self.nb05
            .as_deref()
            .ok_or_else(|| fdo::Error::NotSupported("no NB05 platform".into()))
    }
}

#[zbus::interface(name = "com.tuxedo.Daemon.Fan")]
impl FanInterface {
    async fn set_fan_speed(&self, fan_index: u32, pwm: u8) -> fdo::Result<()> {
        let nb05 = self.require_nb05()?;
        nb05.fan_ctl
            .set_fan_pwm(fan_index as u8, pwm)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn set_auto_mode(&self, fan_index: u32) -> fdo::Result<()> {
        let nb05 = self.require_nb05()?;
        nb05.fan_ctl
            .set_fan_auto(fan_index as u8)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn get_fan_speed(&self, fan_index: u32) -> fdo::Result<u32> {
        use crate::nb05::sensors;
        let nb05 = self.require_nb05()?;
        let rpm = match fan_index {
            0 => sensors::read_fan1_rpm(nb05.fan_ctl.ec()),
            1 => sensors::read_fan2_rpm(nb05.fan_ctl.ec()),
            _ => return Err(fdo::Error::InvalidArgs("invalid fan index".into())),
        };
        rpm.map(|r| r as u32)
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn get_temperature(&self, sensor_index: u32) -> fdo::Result<i32> {
        use crate::nb05::sensors;
        let nb05 = self.require_nb05()?;
        if sensor_index != 0 {
            return Err(fdo::Error::InvalidArgs("only sensor 0 (CPU) supported".into()));
        }
        sensors::read_cpu_temp(nb05.fan_ctl.ec())
            .map(|t| t as i32 * 1000) // millidegrees
            .map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    async fn get_fan_info(&self) -> fdo::Result<(u32, u32, bool, u8)> {
        let nb05 = self.require_nb05()?;
        let limits = crate::nb05::sensors::fan_limits(&nb05.product_sku);
        Ok((
            limits.max_rpm as u32,
            limits.min_rpm as u32,
            nb05.fan_ctl.num_fans() > 1,
            nb05.fan_ctl.num_fans(),
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
    current_profile: String,
    available_profiles: Vec<String>,
}

impl ProfileInterface {
    fn new(config: &Config) -> Self {
        Self {
            current_profile: config.profile.default.clone(),
            available_profiles: vec![
                "powersave".into(),
                "balanced".into(),
                "performance".into(),
            ],
        }
    }
}

#[zbus::interface(name = "com.tuxedo.Daemon.Profile")]
impl ProfileInterface {
    async fn set_profile(&self, profile: &str) -> fdo::Result<()> {
        let _ = profile;
        Err(fdo::Error::NotSupported("not yet implemented".into()))
    }

    #[zbus(property)]
    async fn current_profile(&self) -> &str {
        &self.current_profile
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
