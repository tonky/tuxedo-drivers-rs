# Phase 0: Foundation — DONE

**Goal**: Daemon skeleton, D-Bus, DMI detection, config, systemd service.

## What Was Built

```
tuxedo-daemon/src/
├── main.rs          tokio runtime, signal handling, systemd watchdog, platform init
├── config.rs        TOML config: fan mode, curve points, validation
├── dbus.rs          zbus 5.x D-Bus server (Fan + Device interfaces)
├── dmi.rs           DMI detection: NB05, NB04, Uniwill, Clevo, Tuxi platforms
└── (systemd)        tuxedo-daemon.service with Type=notify, WatchdogSec=30
```

## Key Decisions

- **D-Bus over ioctl**: Replaces `/dev/tuxedo_io` char device with `com.tuxedo.Daemon`
- **Platform enum**: `{Nb05, Nb04, Uniwill, Clevo, Tuxi}` — detected via DMI fields + WMI GUIDs
- **Fan curve config**: TOML with `[[fan.curve]]` temp/speed pairs, hysteresis, min_speed_percent
- **sd-notify 0.5 API**: `sd_notify::notify(&[...])` — no boolean first arg
- **zbus 5.x**: Requires `features = ["tokio"]` explicitly

## Tests

- DMI matching (NB05 models, Clevo board heuristic)
- Config parsing and validation (bad curves, OOB speeds)
