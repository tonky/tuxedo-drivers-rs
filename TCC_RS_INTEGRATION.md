# tcc-rs Integration Notes

Analysis of `/home/tonky/projects/tcc-rs/` — Rust rewrite of TUXEDO Control Center.

## Architecture

tcc-rs is a standalone daemon + TUI, no dependency on tuxedo-drivers-rs:

- **tccd-daemon**: tokio + zbus 5 D-Bus service, handles hardware I/O and profile management
- **tccd-tui**: ratatui terminal UI, connects to daemon over D-Bus (TEA architecture)
- **Hardware access**: Pure sysfs (no ioctls, no `/dev/tuxedo_io`, no kernel shims)
- **IO abstraction**: `TuxedoIO` trait in `tccd-daemon/src/io.rs` (20 methods), with `SysFsTuxedoIO` and `MockTuxedoIO`

## D-Bus Interface Comparison

| | tuxedo-drivers-rs | tcc-rs |
|---|---|---|
| Bus name | `com.tuxedo.Daemon` | `com.tuxedocomputers.tccd` |
| Object path | `/com/tuxedo/Daemon` | `/com/tuxedocomputers/tccd` |
| Interfaces | 4 typed (Fan, Keyboard, Profile, Device) | 1 flat (28 methods) |
| Data format | Native D-Bus types | JSON strings over D-Bus |
| Bus type | System bus | System (default) or session (dev mode) |

## tcc-rs D-Bus Methods (28 total)

### Fan Control
- `SetFanSpeedPercent(speed: u8)` — manual fan speed override (0-100%)
- `GetFanSpeedPercent() -> u8`
- `GetActiveFanCurve() -> String` — JSON with profile name + fan curve points
- `SetFanCurve(json: String)` — JSON array of `{temp, speed}` points

### Profile Management
- `ListProfiles() -> String` — JSON array of all profiles
- `GetProfile(id: String) -> String` — full profile JSON
- `CreateProfile(json: String) -> String` — returns new ID
- `UpdateProfile(id: String, json: String)`
- `DeleteProfile(id: String)`
- `CopyProfile(id: String) -> String`
- `SetActiveProfile(id: String, state: String)` — state is "power_ac" or "power_bat"
- `GetProfileAssignments() -> String` — JSON TccSettings with stateMap

### Telemetry
- `GetCpuInfo() -> String` — JSON `{temperature, avgFrequencyMhz, coreCount}`
- `GetPowerState() -> String` — "ac" or "battery"
- `GetSystemInfo() -> String` — JSON `{tccVersion, daemonVersion, hostname, kernelVersion}`
- `GetGpuInfo() -> String` — JSON with dGPU/iGPU names, temps, usage, PRIME mode

### Settings
- `GetGlobalSettings() -> String` — JSON `{fahrenheit, cpuSettingsEnabled, fanControlEnabled, stateMap}`
- `SetGlobalSettings(json: String)`

### Keyboard
- `GetKeyboardState() -> String` — JSON `{brightness, color, mode}`
- `SetKeyboardState(json: String)`

### Charging
- `GetChargingSettings() -> String` — JSON `{chargingProfile, chargingPriority, startThreshold, endThreshold}`
- `SetChargingSettings(json: String)`

### Power / Shutdown
- `GetPowerSettings() -> String` — JSON `{primeMode, tgpOffset, shutdownHours, shutdownMinutes, shutdownActive}`
- `SetPowerSettings(json: String)`
- `ScheduleShutdown(hours: u32, minutes: u32)`
- `CancelShutdown()`

### Display
- `GetDisplayModes() -> String` — JSON with brightness, refresh rates, resolutions
- `SetDisplaySettings(json: String)`

### Webcam
- `ListWebcamDevices() -> String` — JSON array of `{path, name}`
- `GetWebcamControls(device: String) -> String`
- `SetWebcamControls(device: String, json: String)`

## Feature Overlap

| Feature | tuxedo-drivers-rs | tcc-rs |
|---------|-------------------|--------|
| Fan PWM read/write | Via kernel shims (safe) | Direct sysfs (no shim safety) |
| Fan curve engine | FanBackend trait, 2s poll | FanControlTask, 20%/tick smoothing |
| Temperature sensors | Per-platform adapters | hwmon/thermal zone scanning |
| Keyboard backlight | ITE HID + NB05 EC | Sysfs LED class |
| Power profiles | NB04 only | Full profile CRUD + AC/battery auto-switch |
| Charging | Not implemented | Sysfs battery thresholds |
| GPU info | Not implemented | PCI/sysfs enumeration |
| Display | Not implemented | Sysfs backlight |
| Webcam | Not implemented | V4L2 sysfs |

## Merge Strategy (Future)

**Recommended: tcc-rs consumes tuxedo-drivers-rs shims**

1. Replace `SysFsTuxedoIO` in tcc-rs with reads from our kernel shim sysfs paths
   (e.g., `/sys/devices/platform/tuxedo-uw-fan/` instead of raw hwmon scanning)
2. This gives tcc-rs the safety guarantees: auto-restore on crash, mutex-protected
   EC access, validated probe sequences
3. tcc-rs keeps its own fan curve engine, profile management, and UI features
4. Our daemon becomes optional (or merges into tccd-daemon as a library crate)

**Alternative: tcc-rs calls our D-Bus API**

Would require aligning interfaces: either tcc-rs adopts `com.tuxedo.Daemon.*`
or we add a compatibility layer. Less clean than the library approach.

## Key tcc-rs Source Files

| File | Purpose |
|------|---------|
| `tccd-daemon/src/main.rs` | All 28 D-Bus methods |
| `tccd-daemon/src/io.rs` | `TuxedoIO` trait — the integration point |
| `tccd-daemon/src/profiles.rs` | Profile data types, JSON persistence |
| `tccd-daemon/src/workers/fan.rs` | Fan control loop |
| `tccd-daemon/src/workers/power.rs` | AC/battery auto-switch |
| `tccd-tui/src/dbus_client.rs` | D-Bus proxy (all 28 method calls) |
