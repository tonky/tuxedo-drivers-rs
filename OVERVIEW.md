# tuxedo-drivers-rs: Architecture Overview

## Goal

Migrate tuxedo-drivers from 26 out-of-tree C kernel modules (~15K LOC) to a
hybrid architecture: minimal C kernel shims + Rust userspace daemon. Fully
self-contained PoC — no upstream kernel driver dependencies.

## Source Repository

Original C drivers: `vendor/tuxedo-drivers/` (v4.21.3)

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                  Userspace  (Rust)                        │
│                                                          │
│  tuxedo-daemon (systemd service, tokio async)            │
│  ├── fan_curve   FanBackend trait + curve engine          │
│  ├── nb05/       NB05 EC: fan, sensors, kbd backlight    │
│  ├── uniwill/    Uniwill: fan via EC shim sysfs          │
│  ├── tuxi/       Tuxi: fan via ACPI TFAN shim sysfs      │
│  ├── clevo/     Clevo: fan via WMI/ACPI DSM shim sysfs   │
│  ├── nb04/      NB04: sensors + power profiles (WMI)    │
│  ├── hid/        ITE keyboard LEDs (5 families, hidraw)  │
│  ├── dbus        D-Bus API (com.tuxedo.Daemon)           │
│  ├── config      TOML fan curves, keyboard settings      │
│  └── dmi         Platform detection (DMI + WMI GUIDs)    │
│                                                          │
├──────────────────────────────────────────────────────────┤
│  Kernel interfaces: sysfs, hidraw, pread/pwrite          │
├──────────────────────────────────────────────────────────┤
│               Kernel  (C, GPLv2)                         │
│                                                          │
│  tuxedo-ec       NB05 EC port I/O (0x4e/0x4f)            │
│  └── sysfs: /sys/devices/platform/tuxedo-ec/ec_ram       │
│                                                          │
│  tuxedo-uw-fan   Uniwill EC via ACPI ECRR/ECRW           │
│  └── sysfs: /sys/devices/platform/tuxedo-uw-fan/         │
│                                                          │
│  tuxedo-tuxi     Tuxi ACPI TFAN methods                  │
│  └── sysfs: /sys/devices/platform/tuxedo-tuxi/           │
│                                                          │
│  tuxedo-clevo   Clevo WMI + ACPI DSM dual-transport       │
│  └── sysfs: /sys/devices/platform/tuxedo-clevo/          │
│                                                          │
│  tuxedo-nb04    NB04 WMI BS: sensors + power profiles    │
│  └── sysfs: /sys/devices/platform/tuxedo-nb04/           │
│                                                          │
│  (ITE HID drivers removed — userspace hidraw)            │
│  (tuxedo_io removed — replaced by D-Bus)                 │
└──────────────────────────────────────────────────────────┘
```

## Self-Contained Kernel Shims

Each platform gets a thin kernel module that exposes raw hardware access via
sysfs. All policy logic (fan curves, thermal safety, profile management) lives
in the Rust daemon. No upstream kernel driver dependencies.

| Shim | Platform | Hardware Access | sysfs Path |
|------|----------|----------------|------------|
| `tuxedo-ec` | NB05 | EC port I/O (0x4e/0x4f) | `/sys/devices/platform/tuxedo-ec/` |
| `tuxedo-uw-fan` | Uniwill | ACPI ECRR/ECRW methods | `/sys/devices/platform/tuxedo-uw-fan/` |
| `tuxedo-tuxi` | Tuxi | ACPI TFAN methods | `/sys/devices/platform/tuxedo-tuxi/` |
| `tuxedo-clevo` | Clevo | WMI/ACPI DSM dual-transport | `/sys/devices/platform/tuxedo-clevo/` |
| `tuxedo-nb04` | NB04 | WMI BS methods | `/sys/devices/platform/tuxedo-nb04/` |

## Module Disposition

| Current Module(s) | Count | Disposition |
|--------------------|-------|-------------|
| ite_8291, ite_8291_lb, ite_8297, ite_829x | 4 | **Removed.** Userspace hidraw. |
| tuxedo_nb05_ec | 1 | **Replaced** by `tuxedo-ec` (~180 LOC). |
| tuxedo_nb05_fan_control, _sensors, _kbd_backlight, _keyboard, _power_profiles | 5 | **Removed.** Rust daemon via `tuxedo-ec` sysfs. |
| uniwill_wmi, tuxedo_keyboard | 2 | **Removed.** Rust daemon via `tuxedo-uw-fan` shim. |
| tuxedo_tuxi_fan_control, tuxi_acpi | 2 | **Removed.** Rust daemon via `tuxedo-tuxi` shim. |
| clevo_wmi, clevo_acpi | 2 | **Replaced** by `tuxedo-clevo` shim (~290 LOC). |
| tuxedo_nb04_wmi_ab, _wmi_bs, _sensors, _kbd_backlight, _keyboard, _power_profiles | 6 | **Replaced** by `tuxedo-nb04` shim (~220 LOC). Sensors + profiles only. |
| tuxedo_io | 1 | **Removed.** Replaced by D-Bus. |
| tuxedo_compatibility_check | 1 | **Removed.** DMI matching in Rust. |
| stk8321 | 1 | Keep & upstream (IIO, InfinityFlex only). |
| gxtp7380 | 1 | Keep & upstream (ACPI uevent, InfinityFlex only). |
| tuxedo_nb02_nvidia_power_ctrl | 1 | Deferred. Use nvidia-smi / sysfs. |

**Current result: 26 modules → 5 kernel shims + 1 Rust daemon**
(+ stk8321, gxtp7380 to upstream separately)

## Phases

```
Phase 0: Foundation ─────────────────────── DONE
   │
   ├──→ Phase 1: ITE HID (USB keyboard) ── DONE
   │
   ├──→ Phase 2: NB05 EC (shim + fans) ─── DONE
   │
   └──→ Phase 3: Fan framework + ───────── DONE
        Uniwill + Tuxi adapters
                │
                └──→ Phase 4: Remaining ── DONE
                     4a: Clevo (WMI/ACPI DSM)
                     4b: NB04 (sensors + profiles)
                     4c: Integration & polish
```

## Codebase Stats

- **22 Rust source files** in `tuxedo-daemon/src/`
- **5 kernel shims** (~1170 LOC C total)
- **56 unit tests**, all passing
- **0 external upstream kernel dependencies**

## Key Files

| File | Role |
|------|------|
| `tuxedo-daemon/src/fan_curve.rs` | `FanBackend` trait, fan curve engine |
| `tuxedo-daemon/src/main.rs` | Platform detection, backend wiring, shutdown |
| `tuxedo-daemon/src/dbus.rs` | D-Bus server (Fan, Keyboard, Profile, Device) |
| `tuxedo-daemon/src/dmi.rs` | Platform enum, DMI/WMI detection |
| `tuxedo-daemon/src/clevo/mod.rs` | Clevo adapter (FANINFO parsing, FanBackend) |
| `tuxedo-daemon/src/nb04/mod.rs` | NB04 adapter (sensors, power profiles) |
| `tuxedo-daemon/src/ec.rs` | NB05 EC client (pread/pwrite) |
| `tuxedo-ec-kmod/tuxedo_ec.c` | NB05 EC port I/O kernel shim |
| `tuxedo-uw-fan-kmod/tuxedo_uw_fan.c` | Uniwill EC access kernel shim |
| `tuxedo-tuxi-kmod/tuxedo_tuxi.c` | Tuxi ACPI TFAN kernel shim |
| `tuxedo-clevo-kmod/tuxedo_clevo.c` | Clevo WMI/ACPI DSM kernel shim |
| `tuxedo-nb04-kmod/tuxedo_nb04.c` | NB04 WMI BS kernel shim |

## PoC Scope: In vs Out

**In scope (fan control + sensors for each platform):**
- Fan PWM read/write via kernel shim sysfs
- Temperature reads
- Fan RPM reads (where hardware provides them)
- Fan curve engine (generic, works with any FanBackend)
- ITE keyboard backlight (all 5 families)
- NB05 keyboard backlight via EC
- D-Bus exposure of all endpoints
- Systemd service with watchdog + graceful shutdown

**Out of scope (deferred post-PoC):**
- Keyboard backlight for NB04/Clevo/Tuxi (WMI-based)
- WMI event handling (hotkeys, brightness keys)
- Power profiles (except NB04 where it IS the fan mechanism)
- Charge control, TDP, webcam/touchpad/flightmode switches
- Uniwill universal fan table programming (EC 0x0f00-0x0f5f)
- NVIDIA GPU power control
- Suspend/resume handling
- DKMS packaging and distribution
- TUXEDO Control Center GUI compatibility
