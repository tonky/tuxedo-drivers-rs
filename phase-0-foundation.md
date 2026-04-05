# Phase 0: Foundation

**Goal**: Set up the Rust project, D-Bus interface, and systemd service skeleton.
No hardware interaction yet — just the scaffolding.

**Depends on**: Nothing.

## Tasks

### 0.1 — Project structure

```
tuxedo-drivers-rs/
├── Cargo.toml              (workspace)
├── tuxedo-daemon/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         (tokio + signal handling)
│       ├── dbus.rs         (zbus D-Bus server)
│       ├── config.rs       (TOML config loading)
│       └── dmi.rs          (device detection)
├── tuxedo-ec-kmod/         (C kernel module, phase 2)
│   ├── Makefile
│   ├── dkms.conf
│   └── tuxedo_ec.c
├── docs/migration/
└── README.md
```

### 0.2 — Core dependencies (Cargo.toml)

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
zbus = "5"                    # D-Bus async
serde = { version = "1", features = ["derive"] }
toml = "0.8"                  # config file parsing
tracing = "0.1"               # structured logging
tracing-subscriber = "0.3"
nix = { version = "0.29", features = ["ioctl", "fs", "uio"] }  # Linux syscalls (uio for pread/pwrite)
# DMI: read directly from /sys/class/dmi/id/{board_vendor,product_sku,...}
# No external crate needed — it's just reading sysfs text files.
```

### 0.3 — DMI device detection (replaces tuxedo_compatibility_check)

Port the DMI matching tables from `tuxedo-drivers/src/tuxedo_compatibility_check/`.
Read from `/sys/class/dmi/id/` sysfs entries. Build a device registry:

```rust
pub struct TuxedoDevice {
    pub name: &'static str,
    pub board_vendor: &'static str,
    pub product_sku: &'static str,
    pub platform: Platform,  // NB04, NB05, Clevo, Uniwill, Tuxi
    pub features: DeviceFeatures,
}

pub struct DeviceFeatures {
    pub num_fans: u8,
    pub has_rgb_keyboard: bool,
    pub keyboard_type: KeyboardType,  // IteHid, ClevoWmi, UniwillWmi
    pub has_tdp_control: bool,
    pub has_charging_profiles: bool,
    // ...
}
```

Source of truth: DMI tables in
- `tuxedo-drivers/src/tuxedo_compatibility_check/tuxedo_compatibility_check.c`
- `tuxedo-drivers/src/tuxedo_nb05/tuxedo_nb05_ec.c` (NB05 device table)
- `tuxedo-drivers/src/tuxedo_nb04/tuxedo_nb04_wmi_ab.c` (NB04 device table)
- `tuxedo-drivers/src/uniwill_interfaces.h` (Uniwill feature detection)

### 0.4 — D-Bus interface definition

Define `com.tuxedo.Daemon` D-Bus service with interfaces:

```xml
<!-- com.tuxedo.Daemon.Fan -->
<!-- PWM values are always 0-255 (hwmon convention). The daemon handles   -->
<!-- conversion to platform-specific scales (NB05 duty 0-100%, etc.)     -->
<interface name="com.tuxedo.Daemon.Fan">
  <method name="SetFanSpeed">
    <arg name="fan_index" type="u" direction="in"/>
    <arg name="pwm" type="y" direction="in"/>  <!-- 0-255, daemon applies deadband/floor -->
  </method>
  <method name="SetAutoMode">
    <arg name="fan_index" type="u" direction="in"/>
  </method>
  <method name="GetFanSpeed">
    <arg name="fan_index" type="u" direction="in"/>
    <arg name="rpm" type="u" direction="out"/>
  </method>
  <method name="GetTemperature">
    <arg name="sensor_index" type="u" direction="in"/>
    <arg name="millidegrees" type="i" direction="out"/>  <!-- millidegrees C, matches hwmon -->
  </method>
  <method name="GetFanInfo">
    <arg name="fan_count" type="u" direction="out"/>
    <arg name="sensor_count" type="u" direction="out"/>
    <arg name="supports_manual" type="b" direction="out"/>
    <arg name="min_pwm" type="y" direction="out"/>  <!-- minimum non-zero PWM (deadband) -->
  </method>
  <property name="FanCount" type="u" access="read"/>
  <signal name="FanSpeedChanged">
    <arg name="fan_index" type="u"/>
    <arg name="rpm" type="u"/>
  </signal>
  <signal name="TemperatureChanged">
    <arg name="sensor_index" type="u"/>
    <arg name="millidegrees" type="i"/>
  </signal>
</interface>

<!-- com.tuxedo.Daemon.Keyboard -->
<interface name="com.tuxedo.Daemon.Keyboard">
  <method name="SetBrightness">
    <arg name="brightness" type="y" direction="in"/>  <!-- 0-50 for ITE, 0-255 for others -->
  </method>
  <method name="SetColor">
    <arg name="zone" type="u" direction="in"/>
    <arg name="r" type="y" direction="in"/>
    <arg name="g" type="y" direction="in"/>
    <arg name="b" type="y" direction="in"/>
  </method>
  <method name="SetMode">
    <arg name="mode" type="s" direction="in"/>  <!-- "static", "breath", "wave", etc. -->
  </method>
  <method name="GetKeyboardInfo">
    <arg name="type" type="s" direction="out"/>       <!-- "ite_perkey", "ite_zones", "wmi", "ec" -->
    <arg name="max_brightness" type="y" direction="out"/>
    <arg name="zone_count" type="u" direction="out"/>
    <arg name="modes" type="as" direction="out"/>     <!-- available animation modes -->
  </method>
  <signal name="BrightnessChanged">
    <arg name="brightness" type="y"/>
  </signal>
</interface>

<!-- com.tuxedo.Daemon.Profile -->
<interface name="com.tuxedo.Daemon.Profile">
  <method name="SetProfile">
    <arg name="profile" type="s" direction="in"/> <!-- "powersave", "balanced", "performance" -->
  </method>
  <property name="CurrentProfile" type="s" access="read"/>
  <property name="AvailableProfiles" type="as" access="read"/>
  <signal name="ProfileChanged">
    <arg name="profile" type="s"/>
  </signal>
</interface>

<!-- com.tuxedo.Daemon.Device -->
<interface name="com.tuxedo.Daemon.Device">
  <property name="DeviceName" type="s" access="read"/>
  <property name="Platform" type="s" access="read"/>
  <property name="DaemonVersion" type="s" access="read"/>
  <signal name="DeviceHotplug">
    <arg name="subsystem" type="s"/>  <!-- "hid", "hwmon", etc. -->
    <arg name="action" type="s"/>     <!-- "add", "remove" -->
  </signal>
</interface>
```

This replaces the `tuxedo_io` ioctl interface (`/dev/tuxedo_io` with magic 0xEC).
Map each ioctl from `tuxedo_io_ioctl.h` to a D-Bus method.

### 0.5 — Systemd service

```ini
[Unit]
Description=TUXEDO Hardware Daemon
After=dbus.service

[Service]
Type=dbus
BusName=com.tuxedo.Daemon
ExecStart=/usr/bin/tuxedo-daemon
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

### 0.6 — Config file

```toml
# /etc/tuxedo-daemon/config.toml

[fan]
mode = "auto"           # "auto", "manual", "custom-curve"
min_speed_percent = 25  # minimum non-zero fan speed (deadband threshold)

# Fan curve: temp in °C, speed in percent (0-100%).
# The daemon converts to platform-specific units internally:
#   - NB05 EC: duty 0-100 (direct)
#   - hwmon/sysfs: PWM 0-255
# Deadband applies: speeds in (0, min_speed_percent/2] → off,
#                   speeds in (min_speed_percent/2, min_speed_percent) → min_speed_percent
[[fan.curve]]
temp = 40
speed = 0
[[fan.curve]]
temp = 60
speed = 30
[[fan.curve]]
temp = 80
speed = 80
[[fan.curve]]
temp = 90
speed = 100

[keyboard]
mode = "static"
brightness = 50
color = "#ffffff"

[profile]
default = "balanced"
```

## Deliverable

A `tuxedo-daemon` binary that:
- Starts as systemd service
- Detects device via DMI
- Exposes D-Bus interfaces (methods return "not implemented" stubs)
- Loads/saves config from TOML
- Logs via tracing

No hardware interaction. That comes in phases 1-3.
