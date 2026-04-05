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

## Project Status

Proof-of-concept, not yet tested on hardware. See [HARDWARE.md](HARDWARE.md) for testing details and [RISKS.md](RISKS.md) for known risks.

## License

- Kernel shims: GPL-2.0-only
- Rust daemon: GPL-2.0-only
