# Worklog

## 2026-04-05 — Phase 0: Foundation

### What was done
- Created Cargo workspace with `tuxedo-daemon` crate
- Dependencies: tokio, zbus 5, serde, toml, tracing, nix, anyhow
- Implemented `dmi.rs`: DMI sysfs detection for NB05 (Pulse 14 Gen3/Gen4, InfinityFlex 14), NB04, Uniwill, Clevo, Tuxi platforms
- Implemented `config.rs`: TOML config with fan curves (mode/min_speed/curve points), keyboard settings, profile defaults. Validates curve ordering and value ranges.
- Implemented `dbus.rs`: zbus D-Bus server with stub interfaces — Fan, Keyboard, Profile, Device. All methods return "not yet implemented". Properties work (device name, platform, version, profiles).
- Implemented `main.rs`: tokio entrypoint, tracing init, DMI detection, config load, D-Bus server launch with ctrl-c shutdown.
- Created `dist/tuxedo-daemon.service` systemd unit.

### Key decisions
- Used zbus 5 (latest async D-Bus for Rust) on system bus
- DMI detection reads `/sys/class/dmi/id/` sysfs directly (no external crate)
- NB05 devices identified by `board_vendor == "NB05"` + `product_sku`
- Clevo detection is heuristic by board name prefix — WMI GUID check is authoritative (Phase 3)
- Default unknown TUXEDO device → Uniwill platform (most common)

### Test results
- 9 unit tests passing (DMI matching, config parsing/validation)
- clippy clean (only expected dead-code warnings for future-phase types)

## 2026-04-05 — Phase 1: ITE HID Keyboard LED Control

### What was done
- Implemented `hid/hidraw.rs`: low-level hidraw ioctl interface using nix macros (HIDIOCGRAWINFO, HIDIOCSFEATURE, HIDIOCGFEATURE), device enumeration, VID/PID filtering, USB sysfs bcdDevice reading
- Implemented `hid/mod.rs`: `Rgb` struct, `KeyboardLed` trait (set_brightness, set_color, zone_count, turn_off, turn_on, flush, device_type, available_modes)
- Implemented `hid/ite8291.rs`: ITE 8291 per-key (6x21, 126 LEDs) and zones (4 zones) controller. Variant detection by PID/bcdDevice. Row data: 65-byte interrupt OUT with B-G-R byte ordering. PIDs: [0xce00, 0x6004, 0x600a, 0x600b]
- Implemented `hid/ite8291_lb.rs`: ITE 8291 lightbar with 3 sub-variants (Standard 0x6010, Enhanced 0x7000, Variant7001 0x7001). Supports mono, breathe, and variant-specific animation modes
- Implemented `hid/ite8297.rs`: Simple RGB lightbar, 64-byte feature reports with 0xcc/0xb0 command prefix. PID: 0x8297
- Implemented `hid/ite829x.rs`: Per-key keyboard 6x20, 6-byte feature reports. PID: 0x8910. LED ID encoding: `((row & 0x07) << 5) | (col & 0x1f)`
- Implemented `hid/color_scaling.rs`: Per-model RGB correction ported from C driver. 10+ model-specific entries with row-dependent corrections. Default: green=126/255, blue=120/255
- Implemented `hid/discover.rs`: Device scanning and initialization, returns `Box<dyn KeyboardLed>`
- Wired D-Bus `KeyboardInterface` to HID: SetBrightness, SetColor, GetKeyboardInfo all functional

### Fixes applied during review
- Fixed `ite8297::turn_off` — was sending all-zero 64-byte report (wrong protocol), now sends proper `[0xcc, 0xb0, 0x01, 0x01, 0, 0, 0, ...]` command with black RGB
- Fixed mutex poisoning in `dbus.rs` — `lock().unwrap()` would panic permanently after any panic, changed to `unwrap_or_else(|e| e.into_inner())`
- Merged duplicate match arms in `ite8291_lb::write_off` (Standard and Variant7001 were identical)
- Fixed `scale()` overflow — was using `u16` math causing overflow on `300 * 255`, changed to `u32`

### Review findings — false positives
- Conformance review claimed B-R-G byte order, but code correctly uses B-G-R (verified against C source `ite_8291.c`)
- Review claimed wrong per-key mode byte 5, but code matches C source exactly
- Review claimed wrong USB PIDs — code matches C driver `ite_8291.c`, `ite_8297.c`, `ite_829x.c`
- Root cause: the spec document (phase-1-ite-hid.md) had inaccuracies vs the C source; code was written from C source, not the spec

### Key decisions
- Used raw hidraw ioctls via nix instead of libhidapi (no C dependency)
- Only first discovered HID device is opened (multi-device support deferred)
- Animation engine deferred — hardware built-in modes supported, software tick-based animations later
- No HID interface/usage page filtering yet (composite USB device handling deferred)

### Test results
- 16 unit tests passing (color scaling, LED encoding, bcdDevice parsing + Phase 0 tests)
- clippy clean
