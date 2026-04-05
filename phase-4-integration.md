# Phase 4: Remaining Platforms + Integration

**Goal**: Add Clevo and NB04 platform support, then polish for PoC completion.

**Status**: DONE (4a Clevo, 4b NB04, 4c Integration all complete)

## 4a: Clevo Platform — Fan Control via WMI/ACPI DSM

**New kernel shim** (`tuxedo-clevo-kmod/tuxedo_clevo.c`, ~280 LOC):

Dual-transport driver:
1. Try ACPI DSM first: HID `CLV0001`, UUID `93f224e4-fbdc-4bbf-add6-db71bdc0afad`
2. Fall back to WMI: GUID `ABBC0F6D-8EA1-11D1-00A0-C90629100000`
3. Validate on probe: cmd 0x52 (GET_BIOS_FEATURES_1) must return non-0xffffffff

Internal: `clevo_cmd(cmd, arg) -> u32` dispatching to chosen transport.

**sysfs** at `/sys/devices/platform/tuxedo-clevo/`:
- `fan0_info` (R) — cmd 0x63, returns raw u32 (FANINFO1)
- `fan1_info` (R) — cmd 0x64, returns raw u32 (FANINFO2)
- `fan2_info` (R) — cmd 0x6e, returns raw u32 (FANINFO3)
- `fan_speed` (W) — cmd 0x68, takes packed u32 (3 fans × 1 byte each)
- `fan_auto` (W) — cmd 0x69

On module unload: cmd 0x69 (auto). DMI match: TUXEDO vendor.

**Rust adapter** (`tuxedo-daemon/src/clevo/mod.rs`, ~200 LOC):

Parses FANINFO u32 in Rust:
- bits [7:0] = fan duty (0-255)
- bits [15:8] = temperature (degrees C)
- bits [31:16] = RPM (raw value, may need ×100 — verify on hardware)

Implements `FanBackend`:
- `read_temp()` — parse temp field from `fan0_info`
- `write_pwm()` — pack speeds into u32, write to `fan_speed`
- `read_pwm()` — parse duty field from `fanN_info`
- `read_fan_rpm()` — parse RPM field from `fanN_info`
- `set_auto()` — write `1` to `fan_auto`
- `num_fans()` — probe at init: try fan0/fan1/fan2_info, count valid returns

Wire into `main.rs`: add to backend chain NB05 → Uniwill → Tuxi → Clevo → None.

**Reference code**: `vendor/tuxedo-drivers/src/clevo_interfaces.h`,
`vendor/tuxedo-drivers/src/clevo_acpi.c`, `vendor/tuxedo-drivers/src/clevo_wmi.c`

**Quirk**: Clevo requires 100ms delay after every WMI write (`msleep(100)`,
"no known ready flag"). The kernel shim must enforce this.

**Scope**: ~280 LOC C, ~200 LOC Rust.

## 4b: NB04 Sensors + Profiles (Stretch)

NB04 has NO direct fan PWM control — fans are governed by profile selection
(BATTERY/HUMAN/BEAST). Cannot fully implement `FanBackend`.

**New kernel shim** (`tuxedo-nb04-kmod/tuxedo_nb04.c`, ~220 LOC):

WMI driver binding to GUID `1F174999-3A4E-4311-900D-7BE7166D5055`:
- `cpu_temp` (R) — WMI method 0x04, returns out[2]
- `gpu_temp` (R) — WMI method 0x06, returns out[2]
- `fan0_rpm` (R) — WMI method 0x02, returns (out[3]<<8)|out[2]
- `fan1_rpm` (R) — WMI method 0x02, returns (out[5]<<8)|out[4]
- `power_profile` (RW) — WMI method 0x07, values 0/1/2

Status byte validation: (out[1]<<8)|out[0] must == 0.

**Rust adapter** (`tuxedo-daemon/src/nb04/mod.rs`, ~120 LOC):
- Sensor reads + profile switching only (no fan PWM)
- Wire into D-Bus `ProfileInterface` for `set_profile()`
- Expose temp/RPM via D-Bus read methods

**Scope**: ~220 LOC C, ~120 LOC Rust. Lower PoC value since no fan control.

## 4c: Integration & Polish — DONE

- Deleted `hwmon.rs` (unused generic sysfs client, superseded by per-platform adapters)
- Removed `KeyboardType` enum from `dmi.rs` (populated but never read)
- Removed dead items: `ClevoPlatform::read_attr()`, `PowerProfile::from_u8()`
- Narrowed `#[allow(dead_code)]`: removed blanket module annotations on `nb05`/`uniwill`,
  replaced with targeted annotations on specific unused-from-production items
- `mod hid` retains blanket annotation (many internal items unused from outside)
- D-Bus `ProfileInterface` wired to NB04 backend (was stub)
- All 5 platforms detected and initialized in `main.rs` with graceful fallback
- Fan backend chain: NB05 → Uniwill → Tuxi → Clevo → None
- Shutdown paths restore auto fan mode for all platforms with fan control
- `cargo build` — 0 warnings
- `cargo test` — 56 tests passing

## Verification Per Sub-Phase

1. `cargo build` — no warnings
2. `cargo test` — all existing + new tests pass
3. On target hardware: `insmod` kernel shim, verify sysfs via `cat`/`echo`
4. Start daemon, verify D-Bus endpoints return correct data
5. On non-matching hardware: daemon gracefully falls back to None backend
