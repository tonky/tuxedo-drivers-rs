# Phase 1: ITE HID Keyboard LEDs — DONE

**Goal**: Replace 4 ITE kernel modules with userspace hidraw control.

**Replaced**: `ite_8291`, `ite_8291_lb`, `ite_8297`, `ite_829x` (4 modules, ~2.2K LOC)

## What Was Built

```
tuxedo-daemon/src/hid/
├── mod.rs            Module root, ITE device family enum
├── discover.rs       USB VID/PID enumeration via /dev/hidraw*
├── hidraw.rs         Raw HID feature report + interrupt OUT via nix ioctl
├── color_scaling.rs  Per-model RGB correction tables (SKU + bcdDevice keyed)
├── ite8291.rs        Per-key (6×21) and zone-based RGB, 8-byte feature reports
├── ite8291_lb.rs     Lightbar variant (different HID interface number)
├── ite8297.rs        Zone-based RGB (VID 048d, PID 6004)
└── ite829x.rs        Older variant (VID 048d, PID 5541)
```

## Key Details

- **Two communication paths**: Feature reports (`HIDIOCSFEATURE`) for control,
  interrupt OUT (`write()`) for per-key row data (65-byte packets)
- **Color scaling**: Per-model RGB correction from `color_scaling()` dispatch table.
  Default: R=100%, G≈49%, B≈47%. Model overrides keyed on DMI SKU + USB bcdDevice.
- **Brightness range**: ITE8291 max = 0x32 (50), NOT 0xFF
- **No external C deps**: Raw hidraw via nix, not libhidapi
- **5 device families covered**: per-key, zones, lightbar, 8297, 829x

## Tests

- Variant detection by bcdDevice
- Color scaling per model
- Row data layout (BRG column ordering, 64 bytes)
- Brightness clamping
