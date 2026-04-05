# Phase 1: ITE HID Keyboard LEDs → Userspace

**Goal**: Replace all 4 ITE kernel modules with userspace hidraw control.
This is the safest phase — USB HID is fully accessible from userspace.

**Depends on**: Phase 0 (daemon skeleton, D-Bus interface).

**Replaces**: `ite_8291`, `ite_8291_lb`, `ite_8297`, `ite_829x` (4 kernel modules, ~2.2K LOC)

## Background

The ITE 829x controllers are USB HID devices that control per-key or per-zone
RGB keyboard backlighting. The current kernel drivers use `hid_hw_raw_request()`
with `HID_FEATURE_REPORT` to send 8-byte control messages. This is exactly
what hidraw's `HIDIOCSFEATURE` ioctl does from userspace.

## Tasks

### 1.1 — HID protocol library

Create `tuxedo-daemon/src/hid/` module:

```rust
pub mod ite8291;    // Per-key RGB (6 rows x 21 cols)
pub mod ite8291_lb; // Lightbar variant
pub mod ite8297;    // Zone-based RGB
pub mod ite829x;    // Older variant
```

Port the USB HID protocol from the C drivers. Key reference files:
- `tuxedo-drivers/src/ite_8291/ite_8291.c` lines 80-160 (control messages)
- `tuxedo-drivers/src/ite_8291/ite_8291.c` lines 200+ (animation modes)
- `tuxedo-drivers/src/ite_8297/ite_8297.c` (simpler protocol)

Protocol summary (from ite_8291.c):
```
Control messages are 8-byte HID feature reports:
  [0x08, power_state, anim_mode, speed, brightness, 0x08, behaviour, 0x00]

Power state: 0x01=off, 0x02=on
Anim modes: 0x02=breath, 0x03=wave, 0x04=reactive, 0x05=rainbow, 0x33=per-key
Brightness: 0x00-0x32
Color set:  [0x14, 0x00, color_index, R, G, B, 0x00, 0x00]
Per-key:    [0x16, 0x00, row, 0x00, ...] + 64-byte row data on interrupt EP
```

### 1.2 — Device discovery

Use `hidapi` or raw `hidraw` to find ITE devices:
- ITE8291: USB VID `0x048d`, PID `0xce00` (and variants)
- ITE8297: USB VID `0x048d`, PID `0x6004`
- ITE829x: USB VID `0x048d`, PID `0x5541`

Enumerate `/dev/hidraw*`, read device info, match VID/PID.

Dependencies — two options:
```toml
# Option A: hidapi crate (wraps libhidapi C library — adds a native dep)
hidapi = "2"

# Option B: raw hidraw via nix (no C dependency, more code but fewer build deps)
# Use nix::ioctl_read!/ioctl_write! for HIDIOCGRAWINFO, HIDIOCSFEATURE, etc.
# Recommended: Option B, since we need fine-grained control over feature
# reports vs interrupt OUT (hidapi abstracts this correctly, but raw hidraw
# gives us full visibility and avoids the libhidapi build dependency).
```

### 1.3 — Animation engine

The C drivers implement animations in kernel space (breath, wave, rainbow, etc.).
Move these to userspace with a simple tick-based animation loop:

```rust
pub trait Animation: Send {
    fn tick(&mut self, dt: Duration) -> Vec<KeyColor>;
    fn name(&self) -> &str;
}

pub struct BreathAnimation { /* ... */ }
pub struct WaveAnimation { /* ... */ }
pub struct RainbowAnimation { /* ... */ }
pub struct StaticAnimation { /* ... */ }
pub struct PerKeyAnimation { /* ... */ }
```

Run animations on a separate tokio task, sending frames at ~30fps via hidraw.

### 1.4 — D-Bus Keyboard interface

Wire up `com.tuxedo.Daemon.Keyboard` D-Bus methods to the HID protocol:
- `SetBrightness(u8)` → send brightness control message
- `SetColor(zone, r, g, b)` → send color define message
- `SetMode(string)` → switch animation

### 1.5 — Lightbar support (ite_8291_lb)

The lightbar variant uses the same USB protocol but targets a different
endpoint/interface number. Handle this as a device variant in `ite8291.rs`.

## Appendix: ITE 8291 Protocol Details

The C driver has significant complexity beyond simple 8-byte feature reports.
This appendix captures details that **must** be implemented correctly.

### A1. Two communication paths (feature reports vs interrupt endpoint)

Control commands (brightness, mode select, color define) use **HID feature reports**
via `HIDIOCSFEATURE` ioctl (8-byte buffers). This maps to `hid_send_feature_report()`
in hidapi.

Per-key row data uses the **interrupt OUT endpoint** via `output_report()`. This maps
to `hid_write()` in hidapi (NOT `hid_send_feature_report()`). The protocol:

1. Send feature report: `[0x08, 0x02, 0x33, speed, bright, 0x08, 0x00, 0x00]`
   (select per-key mode 0x33)
2. For each of 6 rows:
   - Send feature report: `[0x16, 0x00, row, 0x00, 0x00, 0x00, 0x00, 0x00]`
     (announce row)
   - Send 65-byte interrupt OUT packet: `[0x00 (padding)] + [64 bytes row data]`
3. Row data layout (64 bytes): BRG column ordering across 21 columns:
   - Bytes 0..20:  Blue  values for columns 0..20
   - Bytes 21..41: Red   values for columns 0..20
   - Bytes 42..62: Green values for columns 0..20
   - Byte 63: unused

### A2. Per-model color scaling table

The C driver applies per-model RGB correction in `color_scaling()`. Without this,
colors will be visibly wrong on every supported model. The default scaling
(applied when no model-specific entry matches) is:

```
red   = red              (100%)
green = (126 * green) / 255  (~49%)
blue  = (120 * blue)  / 255  (~47%)
```

Model-specific overrides exist for: STEPOL1XA04, STELLARIS1XI05, STELLARIS1XA05,
STELLARIS17I06, STELLARIS16I06, X5KK45xS, and others. These are keyed on a
combination of DMI product SKU and `bcd_device` from the USB HID descriptor.

**Action**: Port the full `color_scaling()` dispatch table from
`tuxedo-drivers/src/ite_8291/ite_8291.c`. Read `bcd_device` via hidraw
`HIDIOCGRAWINFO` or from the USB sysfs `bcdDevice` attribute.

### A3. Two sub-variants: per-key vs zones

Both share the same USB VID/PID (`0x048d:0xce00`) but have different capabilities
dispatched via function pointers in the C driver:

- **ite8291_perkey**: 6 rows × 21 columns, full per-key RGB, uses the interrupt
  endpoint path described in A1.
- **ite8291_zones**: 4 color zones, simpler protocol — sets zone colors via
  feature reports only: `[0x14, 0x00, zone_index, R, G, B, 0x00, 0x00]`.

The variant is determined by the `bcd_device` field of the USB descriptor.

### A4. Brightness range

`ITE8291_KBD_BRIGHTNESS_MAX = 0x32` (50 decimal), NOT 0xFF. The D-Bus
`SetBrightness(u8)` method must clamp to this range. Passing values > 0x32
produces undefined hardware behavior.

### A5. Lightbar variant (ite_8291_lb)

Uses the same USB protocol but targets a different HID interface number on the
composite USB device. The device has multiple HID interfaces; the lightbar is
NOT on interface 0. Enumerate all interfaces and match by usage page or
interface number.

## Testing Strategy

- Test on hardware with ITE keyboards (Pulse, Polaris models)
- Verify: can unload kernel ite_8291 module, start daemon, control LEDs
- Compare HID traffic with `usbmon` / `wireshark` between old and new
- Test hot-plug: USB keyboard reconnect should be handled

## Deliverable

The 4 ITE kernel modules can be unloaded and the daemon controls keyboard
LEDs identically via userspace hidraw.

## Rollback

If the daemon crashes, keyboard LEDs stay in their last state (hardware retains
settings). No risk of hardware damage. Users can re-load the kernel modules.
