mod clevo;
mod config;
mod dbus;
mod dmi;
mod ec;
mod fan_curve;
#[allow(dead_code)]
mod hid;
mod nb04;
mod nb05;
mod tuxi;
mod uniwill;

use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "starting tuxedo-daemon");

    let device = dmi::detect_device();
    match &device {
        Some(dev) => info!(name = dev.name, platform = ?dev.platform, "detected TUXEDO device"),
        None => warn!("no known TUXEDO device detected — running in generic mode"),
    }

    let config = config::Config::load_or_default();
    info!(?config.fan.mode, "loaded configuration");

    // Initialize NB05 platform if detected and EC kernel module is loaded
    let nb05 = if let Some(dev) = &device {
        if dev.platform == dmi::Platform::Nb05 && dmi::has_ec_sysfs() {
            match nb05::Nb05Platform::init(dev) {
                Ok(p) => {
                    info!("NB05 platform initialized");
                    // Restore auto mode in case a previous crash left manual mode
                    p.shutdown();
                    Some(Arc::new(p))
                }
                Err(e) => {
                    warn!("failed to initialize NB05 platform: {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Initialize Uniwill platform if detected and fan shim is loaded
    let uniwill = if let Some(dev) = &device {
        if dev.platform == dmi::Platform::Uniwill && uniwill::has_fan_shim() {
            match uniwill::UniwillPlatform::init() {
                Ok(p) => {
                    info!("Uniwill platform initialized");
                    p.shutdown(); // restore auto in case of prior crash
                    Some(Arc::new(p))
                }
                Err(e) => {
                    warn!("failed to initialize Uniwill platform: {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Initialize Tuxi platform if detected and kernel shim is loaded
    let tuxi = if let Some(dev) = &device {
        if dev.platform == dmi::Platform::Tuxi && tuxi::has_shim() {
            match tuxi::TuxiPlatform::init() {
                Ok(p) => {
                    info!("Tuxi platform initialized");
                    p.shutdown(); // restore auto in case of prior crash
                    Some(Arc::new(p))
                }
                Err(e) => {
                    warn!("failed to initialize Tuxi platform: {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Initialize Clevo platform if detected and kernel shim is loaded
    let clevo = if let Some(dev) = &device {
        if dev.platform == dmi::Platform::Clevo && clevo::has_shim() {
            match clevo::ClevoPlatform::init() {
                Ok(p) => {
                    info!("Clevo platform initialized");
                    p.shutdown(); // restore auto in case of prior crash
                    Some(Arc::new(p))
                }
                Err(e) => {
                    warn!("failed to initialize Clevo platform: {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Initialize NB04 platform if detected and kernel shim is loaded
    // NB04 has no fan PWM control — only sensors and power profiles.
    let nb04 = if let Some(dev) = &device {
        if dev.platform == dmi::Platform::Nb04 && nb04::has_shim() {
            match nb04::Nb04Platform::init() {
                Ok(p) => {
                    info!("NB04 platform initialized");
                    Some(Arc::new(p))
                }
                Err(e) => {
                    warn!("failed to initialize NB04 platform: {e}");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Build fan backend: NB05 → Uniwill → Tuxi → Clevo → None
    let fan_backend: Option<Arc<dyn fan_curve::FanBackend>> = if nb05.is_some() {
        nb05.clone().map(|n| n as Arc<dyn fan_curve::FanBackend>)
    } else if uniwill.is_some() {
        uniwill.clone().map(|u| u as Arc<dyn fan_curve::FanBackend>)
    } else if tuxi.is_some() {
        tuxi.clone().map(|t| t as Arc<dyn fan_curve::FanBackend>)
    } else if clevo.is_some() {
        clevo.clone().map(|c| c as Arc<dyn fan_curve::FanBackend>)
    } else {
        info!("no fan backend found, fan control disabled");
        None
    };

    info!("starting D-Bus service");
    let conn = dbus::serve(device, config.clone(), fan_backend.clone(), nb05.clone(), nb04.clone()).await?;

    // Spawn fan curve engine if we have a fan backend
    let fan_engine_handle = {
        if let Some(backend) = fan_backend.clone() {
            let (_config_tx, config_rx) = tokio::sync::watch::channel(config.fan.clone());
            let engine = fan_curve::FanCurveEngine::new(backend, config_rx);
            Some(tokio::spawn(engine.run()))
        } else {
            None
        }
    };

    // Signal systemd that we're ready
    if let Err(e) = sd_notify::notify(&[sd_notify::NotifyState::Ready]) {
        warn!("sd_notify ready failed (not running under systemd?): {e}");
    }

    // Spawn watchdog ping task if WATCHDOG_USEC is set
    if let Ok(usec_str) = std::env::var("WATCHDOG_USEC") {
        if let Ok(usec) = usec_str.parse::<u64>() {
            let interval = std::time::Duration::from_micros(usec / 2);
            info!(?interval, "starting systemd watchdog ping task");
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                loop {
                    ticker.tick().await;
                    let _ = sd_notify::notify(&[sd_notify::NotifyState::Watchdog]);
                }
            });
        }
    }

    // Wait for SIGTERM or SIGINT
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    tokio::select! {
        _ = sigterm.recv() => info!("received SIGTERM"),
        r = tokio::signal::ctrl_c() => {
            r?;
            info!("received SIGINT");
        }
    }

    // Notify systemd we're stopping
    let _ = sd_notify::notify(&[sd_notify::NotifyState::Stopping]);

    // Stop fan curve engine before restoring auto
    if let Some(handle) = fan_engine_handle {
        handle.abort();
    }

    // Restore fans to auto mode before exiting
    if let Some(ref nb05) = nb05 {
        nb05.shutdown();
    }
    if let Some(ref uw) = uniwill {
        uw.shutdown();
    }
    if let Some(ref tx) = tuxi {
        tx.shutdown();
    }
    if let Some(ref cl) = clevo {
        cl.shutdown();
    }

    info!("shutting down");
    drop(conn);
    Ok(())
}
