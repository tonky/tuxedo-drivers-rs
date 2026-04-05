# tuxedo-drivers-rs

Rust rewrite of [tuxedo-drivers](https://github.com/tuxedocomputers/tuxedo-drivers) — 26 out-of-tree C kernel modules replaced by 5 minimal kernel shims + 1 Rust userspace daemon.

## Architecture

```
Userspace:  tuxedo-daemon (Rust, tokio, D-Bus)
               ├── fan curve engine (FanBackend trait)
               ├── ITE keyboard LEDs (hidraw)
               └── platform adapters (NB05, Uniwill, Tuxi, Clevo, NB04)

Kernel:     5 shims (~1170 LOC C total), each exposing raw HW via sysfs
               tuxedo-ec       NB05 EC port I/O
               tuxedo-uw-fan   Uniwill EC (ACPI ECRR/ECRW)
               tuxedo-tuxi     Tuxi ACPI TFAN
               tuxedo-clevo    Clevo WMI/ACPI DSM
               tuxedo-nb04     NB04 WMI BS
```

All policy (fan curves, thermal safety, profiles) lives in userspace. Shims are stateless passthrough.

## Supported Platforms

| Platform | Fan Control | Sensors | Profiles | Keyboard |
|----------|-------------|---------|----------|----------|
| NB05     | PWM via EC  | temp, RPM | deferred | EC backlight |
| Uniwill  | PWM via EC  | temp only (no RPM) | deferred | ITE HID |
| Tuxi     | PWM via ACPI | temp, RPM | deferred | deferred |
| Clevo    | PWM via WMI/DSM | temp, RPM | deferred | deferred |
| NB04     | profile only | temp, RPM | battery/balanced/performance | deferred |

## Prerequisites

- Linux kernel headers (`kernel-devel` / `linux-headers`)
- Rust toolchain (stable)
- A TUXEDO laptop

## Quick Start

### 1. Identify your platform

```bash
cat /sys/class/dmi/id/{sys_vendor,board_vendor,product_sku}
```

### 2. Build and load the kernel shim

Pick the shim for your platform:

```bash
# Uniwill (InfinityBook, Polaris, etc.)
cd tuxedo-uw-fan-kmod
make -C /lib/modules/$(uname -r)/build M=$(pwd) modules
sudo insmod tuxedo_uw_fan.ko

# Verify
ls /sys/devices/platform/tuxedo-uw-fan/
cat /sys/devices/platform/tuxedo-uw-fan/cpu_temp
```

Other shims: `tuxedo-ec-kmod` (NB05), `tuxedo-tuxi-kmod` (Tuxi), `tuxedo-clevo-kmod` (Clevo), `tuxedo-nb04-kmod` (NB04).

### 3. Build and run the daemon

```bash
cargo build
RUST_LOG=debug cargo run
```

The daemon auto-detects the platform from DMI/WMI and logs what it finds.

### 4. Verify via D-Bus

```bash
busctl introspect com.tuxedo.Daemon /com/tuxedo/Daemon
```

### 5. Unload

```bash
# Stop daemon (Ctrl-C or SIGTERM — restores fan auto mode)
sudo rmmod tuxedo_uw_fan  # also restores fan auto mode
```

## Configuration

`/etc/tuxedo-daemon/config.toml` (optional, auto-created with defaults):

```toml
[fan]
mode = "auto"           # auto | manual | custom-curve
min_speed_percent = 25

[[fan.curve]]
temp = 40
speed = 0

[[fan.curve]]
temp = 60
speed = 30

[[fan.curve]]
temp = 75
speed = 60

[[fan.curve]]
temp = 90
speed = 100
```

## Interfaces

### D-Bus (exposes)

Bus name `com.tuxedo.Daemon` on the system bus, object path `/com/tuxedo/Daemon`:

| Interface | Methods / Properties |
|-----------|---------------------|
| `com.tuxedo.Daemon.Fan` | `SetFanSpeed`, `SetAutoMode`, `GetFanSpeed`, `GetTemperature`, `GetFanInfo`, `FanCount` |
| `com.tuxedo.Daemon.Keyboard` | `SetBrightness`, `SetColor`, `SetMode`, `GetKeyboardInfo` |
| `com.tuxedo.Daemon.Profile` | `SetProfile`, `CurrentProfile`, `AvailableProfiles` |
| `com.tuxedo.Daemon.Device` | `DeviceName`, `Platform`, `DaemonVersion` |

### Sysfs (reads/writes)

Platform detection (read-only):

| Path | Purpose |
|------|---------|
| `/sys/class/dmi/id/{sys_vendor,board_vendor,board_name,product_sku,chassis_vendor}` | DMI fields for platform detection |
| `/sys/bus/wmi/devices/{GUID}-N` | WMI GUID presence (NB04, Uniwill) |
| `/sys/bus/acpi/devices/CLV0001:00` | Clevo ACPI device presence |

Kernel shim sysfs (read/write — one active per platform):

| Path | Platform | Attributes |
|------|----------|------------|
| `/sys/devices/platform/tuxedo-ec/ec_ram` | NB05 | Binary EC register access (pread/pwrite) |
| `/sys/devices/platform/tuxedo-uw-fan/` | Uniwill | `cpu_temp`, `gpu_temp`, `fan0_pwm`, `fan1_pwm`, `fan_mode`, `fan_count` |
| `/sys/devices/platform/tuxedo-tuxi/` | Tuxi | `fan0_temp`, `fan1_temp`, `fan0_rpm`, `fan1_rpm`, `fan0_pwm`, `fan1_pwm`, `fan_mode`, `fan_count` |
| `/sys/devices/platform/tuxedo-clevo/` | Clevo | `fan0_info`, `fan1_info`, `fan2_info`, `fan_speed`, `fan_auto` |
| `/sys/devices/platform/tuxedo-nb04/` | NB04 | `cpu_temp`, `gpu_temp`, `fan0_rpm`, `fan1_rpm`, `power_profile` |

### Other files

| Path | Mode | Purpose |
|------|------|---------|
| `/etc/tuxedo-daemon/config.toml` | Read | Fan curves, keyboard settings, default profile |
| `/dev/hidraw*` | Read/Write | ITE keyboard LED control (USB HID feature reports) |
| `/sys/class/hidraw/*/device/` | Read | HID device enumeration (vendor/product ID matching) |

## Project Status

Proof-of-concept, not yet tested on hardware. See [HARDWARE.md](HARDWARE.md) for testing details and [RISKS.md](RISKS.md) for known risks.

## License

- Kernel shims: GPL-2.0-only
- Rust daemon: GPL-2.0-only
