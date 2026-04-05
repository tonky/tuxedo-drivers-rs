# tuxedo-drivers-rs: Migration Plan

## Goal

Migrate tuxedo-drivers from 26 out-of-tree C kernel modules (~15K LOC) to a hybrid
architecture: minimal kernel C shim + Rust userspace daemon, targeting mainline
kernel integration.

## Source Repository

Original C drivers: `vendor/tuxedo-drivers/` (v4.21.3)

## Architecture

```
┌──────────────────────────────────────────────────┐
│              Userspace  (Rust)                    │
│                                                  │
│  tuxedo-daemon (systemd service)                 │
│  ├── fan: Fan curves, thermal policy             │
│  ├── led: RGB LED control (hidraw + animations)  │
│  ├── profile: Power/performance profile mgmt     │
│  ├── dbus: D-Bus API (replaces tuxedo_io ioctl)  │
│  ├── accel: Accelerometer (i2cdev)               │
│  └── compat: DMI device matching                 │
│                                                  │
├──────────────────────────────────────────────────┤
│  Kernel interfaces used:                         │
│  sysfs, hwmon, hidraw, i2cdev, input, LED class  │
├──────────────────────────────────────────────────┤
│              Kernel  (C, GPLv2)                  │
│                                                  │
│  uniwill-laptop  (mainline ≥6.19)                │
│  ├── WMI method calls                            │
│  ├── Hotkey input events                         │
│  └── hwmon sensors / fan sysfs                   │
│                                                  │
│  tuxedo-ec  (small, upstreamable)                │
│  └── NB05 EC port I/O (0x4e/0x4f)               │
│  └── Exposes EC RAM via sysfs or char device     │
│                                                  │
│  (ITE HID drivers removed — userspace hidraw)    │
│  (tuxedo_io removed — replaced by D-Bus)         │
│  (stk8321 removed — userspace i2cdev)            │
└──────────────────────────────────────────────────┘
```

## Modules Disposition

| Current Module(s)                | Count | Disposition                          |
|----------------------------------|-------|--------------------------------------|
| ite_8291, ite_8291_lb,           | 4     | Remove. Replace with hidraw from     |
| ite_8297, ite_829x               |       | userspace daemon.                    |
| clevo_wmi, clevo_acpi,           | 3     | Remove. Rely on upstream             |
| uniwill_wmi                      |       | uniwill-laptop (mainline 6.19+).     |
| tuxedo_keyboard                  | 1     | Remove. Input events from            |
|                                  |       | uniwill-laptop.                      |
| tuxedo_nb04_keyboard,            | 6     | Remove. WMI events handled by        |
| tuxedo_nb04_kbd_backlight,       |       | upstream driver; fan/profile/sensor   |
| tuxedo_nb04_power_profiles,      |       | logic moves to userspace daemon.     |
| tuxedo_nb04_sensors,             |       |                                      |
| tuxedo_nb04_wmi_ab,              |       |                                      |
| tuxedo_nb04_wmi_bs               |       |                                      |
| tuxedo_nb05_ec                   | 1     | Keep. Rewrite as tuxedo-ec, minimal  |
|                                  |       | GPLv2 module for EC port I/O.        |
| tuxedo_nb05_keyboard,            | 5     | Remove. Fan/profile/sensor/backlight |
| tuxedo_nb05_kbd_backlight,       |       | logic moves to userspace daemon      |
| tuxedo_nb05_power_profiles,      |       | talking to tuxedo-ec sysfs.          |
| tuxedo_nb05_sensors,             |       |                                      |
| tuxedo_nb05_fan_control          |       |                                      |
| tuxedo_io                        | 1     | Remove. Replaced by D-Bus API.       |
| tuxedo_compatibility_check       | 1     | Remove. DMI matching in userspace.   |
| stk8321                          | 1     | Keep & upstream. Submit to IIO       |
|                                  |       | subsystem — no upstream equivalent   |
|                                  |       | exists. ~400 LOC, self-contained.    |
| tuxedo_nb02_nvidia_power_ctrl    | 1     | Remove. Use nvidia-smi / sysfs.      |
| gxtp7380                         | 1     | Keep as tiny kmod (~50 LOC). ACPI    |
|                                  |       | uevent relay for touch panel fold    |
|                                  |       | detection. No upstream equivalent.   |
|                                  |       | Submit to platform/x86.             |
| tuxedo_tuxi_fan_control,         | 2     | Remove. Fan control moves to         |
| tuxi_acpi                        |       | userspace via ACPI sysfs.            |

**Result: 26 modules → 3 kernel modules (tuxedo-ec, stk8321, gxtp7380) + 1 mainline (uniwill-laptop)**

Note: stk8321 and gxtp7380 are both small, self-contained, and good candidates
for upstream submission. They only affect InfinityFlex 14 convertible models.
Once upstream, they can be dropped from this project.

## Phases

```
Phase 0: Foundation (daemon skeleton, D-Bus, DMI)
   │
   ├──→ Phase 1: ITE HID (USB keyboard LEDs) ──┐
   │    Independent: USB HID, no kernel module   │
   │                                             │
   ├──→ Phase 2: NB05 EC (kernel shim + fans) ──┼──→ Phase 4: Integration
   │    Independent: EC port I/O, NB05 only      │
   │                                             │
   └──→ Phase 3: WMI/profiles (upstream sysfs) ─┘
        Depends on upstream uniwill-laptop
```

Phases 1, 2, and 3 are **independent** — they target different hardware
subsystems and can be developed concurrently after Phase 0 is complete.
Phase 3 has an external dependency on upstream `uniwill-laptop` maturity,
so starting Phases 1 and 2 first is recommended.

See individual phase files for details:
- [Phase 0: Foundation](phase-0-foundation.md)
- [Phase 1: ITE HID to userspace](phase-1-ite-hid.md)
- [Phase 2: NB05 EC shim + fan/sensor daemon](phase-2-nb05-ec.md)
- [Phase 3: WMI/profile management](phase-3-wmi-profiles.md)
- [Phase 4: Full integration](phase-4-integration.md)

## Key Risks

See [RISKS.md](RISKS.md).
