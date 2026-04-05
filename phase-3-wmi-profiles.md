# Phase 3: WMI/Clevo/Uniwill → Upstream + Userspace

**Goal**: Eliminate the Clevo/Uniwill/NB04/Tuxi kernel modules by relying on
the upstream `uniwill-laptop` driver (mainline ≥6.19) and moving policy logic
to the Rust daemon.

**Depends on**: Phase 0 (daemon skeleton), upstream uniwill-laptop availability.

**Replaces**: `clevo_wmi`, `clevo_acpi`, `uniwill_wmi`, `tuxedo_keyboard`,
`tuxedo_nb04_keyboard`, `tuxedo_nb04_kbd_backlight`, `tuxedo_nb04_power_profiles`,
`tuxedo_nb04_sensors`, `tuxedo_nb04_wmi_ab`, `tuxedo_nb04_wmi_bs`,
`tuxedo_tuxi_fan_control`, `tuxi_acpi`, `tuxedo_io`,
`tuxedo_nb02_nvidia_power_ctrl`, `gxtp7380`
(15 kernel modules, ~11K LOC)

## Upstream Dependencies

### uniwill-laptop (mainline ≥6.19)

References:
- https://github.com/Wer-Wolf/uniwill-laptop
- https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html

This driver handles:
- WMI method calls for Uniwill/TUXEDO hardware
- Hotkey events via input subsystem
- Basic hwmon (temperatures, fan speeds)
- Fan control via sysfs

TUXEDO-specific patches are being submitted to extend it. Track:
- https://gitlab.com/tuxedocomputers/development/packages/tuxedo-drivers/-/issues/138

### What upstream provides vs what we need

| Feature              | Upstream uniwill-laptop | Our daemon needs to add    |
|----------------------|------------------------|----------------------------|
| WMI method calls     | Yes                    | —                          |
| Hotkey input events  | Yes                    | —                          |
| Fan speed read       | Yes (hwmon sysfs)      | —                          |
| Fan speed write      | Yes (sysfs)            | Fan curves, thermal policy |
| Temperature read     | Yes (hwmon sysfs)      | —                          |
| Power profiles       | Partial                | TDP control, custom modes  |
| Keyboard backlight   | Partial (LED class)    | RGB zones, animations      |
| Webcam/touchpad sw   | Maybe                  | D-Bus forwarding           |

## WMI Conflict Resolution Strategy

On kernels >= 6.19, the upstream `uniwill-laptop` module is available but may
not be loaded. The old out-of-tree `tuxedo-drivers` modules register the same
WMI GUIDs. If both are present, only one can bind — the other fails silently.

**Transition plan:**

1. **Detection at daemon startup**: The daemon checks which driver is loaded:
   - `ls /sys/bus/wmi/drivers/uniwill-laptop/` → upstream is active
   - `ls /sys/module/uniwill_wmi/` → old out-of-tree is active
   - Neither → no WMI driver loaded

2. **Kernel >= 6.19 (upstream available)**:
   - Ship a modprobe blacklist: `/etc/modprobe.d/tuxedo-migration.conf`
     ```
     blacklist clevo_wmi
     blacklist clevo_acpi
     blacklist uniwill_wmi
     blacklist tuxedo_keyboard
     blacklist tuxedo_nb04_wmi_ab
     blacklist tuxedo_nb04_wmi_bs
     ```
   - Ensure `uniwill-laptop` loads instead (it's built-in on Fedora,
     autoloads via DMI/WMI aliases on others)
   - The daemon's `UniwillAdapter` reads from upstream's hwmon/sysfs

3. **Kernel < 6.19 (upstream not available)**:
   - Do NOT blacklist old modules
   - The daemon falls back to reading the old modules' sysfs/hwmon outputs
   - This works because the daemon's `HwmonClient` reads standard sysfs
     regardless of which driver created the hwmon device

4. **Clevo devices (no upstream driver)**:
   - Clevo WMI GUID `ABBC0F6D` overlaps with Uniwill management GUID `_BA`
     but they're in different WMI namespaces and won't conflict in practice
   - If no upstream Clevo driver exists: keep `clevo_wmi.c` as a standalone
     minimal module, submit upstream to `platform/x86`
   - The daemon detects Clevo via DMI and uses the appropriate adapter

5. **Package-level conflicts**:
   - `tuxedo-daemon` package: `Conflicts: tuxedo-drivers-dkms (<< 5.0)`
   - Migration package handles unloading old modules and loading new ones

**Key invariant**: The daemon never talks to WMI directly. It always goes
through sysfs/hwmon interfaces exposed by whichever kernel driver is loaded.
This makes the daemon agnostic to the upstream transition.

## Tasks

### 3.1 — Sysfs/hwmon client library

Create `tuxedo-daemon/src/hwmon.rs`:

```rust
pub struct HwmonClient {
    hwmon_path: PathBuf,  // /sys/class/hwmon/hwmonN/
}

impl HwmonClient {
    pub fn find_by_name(name: &str) -> Option<Self>;
    pub fn read_temp(&self, index: u8) -> Result<f64>;
    pub fn read_fan_rpm(&self, index: u8) -> Result<u32>;
    pub fn read_fan_pwm(&self, index: u8) -> Result<u8>;
    pub fn write_fan_pwm(&self, index: u8, pwm: u8) -> Result<()>;
    pub fn write_fan_enable(&self, index: u8, mode: FanMode) -> Result<()>;
}
```

This reads/writes standard hwmon sysfs files:
- `temp1_input`, `temp2_input` (millidegrees C)
- `fan1_input`, `fan2_input` (RPM)
- `pwm1`, `pwm2` (0-255)
- `pwm1_enable`, `pwm2_enable` (0=off, 1=manual, 2=auto)

### 3.2 — Fan curve engine

```rust
pub struct FanCurveEngine {
    hwmon: HwmonClient,
    curves: Vec<FanCurvePoint>,
    poll_interval: Duration,  // e.g. 2 seconds
    hysteresis: f64,          // e.g. 3°C
}

impl FanCurveEngine {
    pub async fn run(&mut self) {
        loop {
            let temp = self.hwmon.read_temp(0)?;
            let target_pwm = self.interpolate(temp);
            self.hwmon.write_fan_pwm(0, target_pwm)?;
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}
```

### 3.3 — Platform-specific adapters

Different platforms (Clevo, Uniwill, NB04, NB05, Tuxi) have different
capabilities. Create a trait-based abstraction:

```rust
pub trait PlatformAdapter: Send + Sync {
    fn fan_control(&self) -> Option<&dyn FanControl>;
    fn keyboard(&self) -> Option<&dyn KeyboardControl>;
    fn profile(&self) -> Option<&dyn ProfileControl>;
    fn sensors(&self) -> Option<&dyn SensorReader>;
}

// Implementations:
pub struct UniwillAdapter { hwmon: HwmonClient }  // uses upstream sysfs
pub struct NB05Adapter { ec: EcClient }           // uses tuxedo-ec (phase 2)
pub struct IteHidAdapter { hid: HidDevice }       // uses hidraw (phase 1)
```

The daemon selects the adapter based on DMI detection (phase 0).

### 3.4 — Power profile management

For platforms where the upstream driver exposes profile control:
- Read available profiles from sysfs
- Map to D-Bus `com.tuxedo.Daemon.Profile` interface
- Integrate with `power-profiles-daemon` if present (via its D-Bus API)

For TDP control (Uniwill-specific):
- If upstream exposes TDP sysfs, use it
- If not, may need a small helper or wait for upstream patches

### 3.5 — Keyboard backlight (Clevo/Uniwill)

For WMI-based keyboard backlights (not USB HID):
- If upstream exposes LED class devices, control via `/sys/class/leds/`
- Wire to `com.tuxedo.Daemon.Keyboard` D-Bus interface

### 3.6 — tuxedo_io ioctl compatibility shim (optional)

For backwards compatibility with existing TUXEDO Control Center:
- Optionally create a FUSE-based `/dev/tuxedo_io` that translates ioctls
  to D-Bus calls
- Or: provide a `libtuxedo-compat.so` that wraps D-Bus calls behind
  the old ioctl API
- **Recommendation**: Skip this. Coordinate with TUXEDO Control Center
  team to migrate to D-Bus directly.

### 3.7 — Clevo-specific handling

Clevo devices use a different WMI GUID than Uniwill. If the upstream
`uniwill-laptop` driver doesn't cover Clevo:
- Check if there's a separate upstream `clevo-laptop` driver
- If not, may need to keep a small Clevo WMI kernel module or
  contribute Clevo support to upstream

Reference: `tuxedo-drivers/src/clevo_interfaces.h` for WMI GUIDs:
- `CLEVO_WMI_METHOD_GUID: ABBC0F6D-8EA1-11D1-00A0-C90629100000`
- `CLEVO_WMI_EVENT_GUID: ABBC0F6B-8EA1-11D1-00A0-C90629100000`

### 3.8 — NVIDIA power control

`tuxedo_nb02_nvidia_power_ctrl` toggles NVIDIA GPU power via ACPI.
Evaluate alternatives:
- `nvidia-smi` or `nvidia-powerd` from NVIDIA's driver
- `/sys/bus/pci/devices/*/power/control` for runtime PM
- `switcheroo` sysfs for hybrid graphics
- If none suffice, keep a minimal ACPI helper

## Phasing Within Phase 3

This is the largest phase. Break it into sub-phases:

1. **3a**: Hwmon/sysfs client + fan curve engine (works on any laptop with
   hwmon, testable without TUXEDO hardware)
2. **3b**: Uniwill adapter using upstream driver sysfs
3. **3c**: NB04 WMI adapter (may need upstream patches)
4. **3d**: Clevo adapter (depends on upstream status)
5. **3e**: Tuxi adapter
6. **3f**: NVIDIA power, touch panel, misc cleanup

## Platform-Specific Quirks

### Clevo 100ms WMI delay

The C driver has `msleep(100)` after every Clevo fan-speed WMI write with the
comment "no known ready flag." The upstream driver (if one materializes) may
or may not handle this. If the daemon ever issues direct WMI calls via a
fallback module, it must enforce this delay. If using upstream sysfs, the
kernel driver handles it.

### Uniwill fan table initialization

`uw_init_fan()` writes a 16-entry temperature-indexed fan table to EC RAM at
0x0f00-0x0f5f on first use, with retry loops. This is part of the "universal
EC fan control" path used by newer Uniwill devices. Verify whether the
upstream `uniwill-laptop` driver performs this initialization — if not, the
daemon must do it before writing fan curves, or fan control will be ignored
by the EC.

### Uniwill TDP model-specific bounds

The `tuxedo_io` Uniwill TDP ioctls clamp values to per-model min/max watt
ranges from a 20+ SKU dispatch table. If the upstream driver exposes TDP via
sysfs, verify it enforces the same bounds. If not, the daemon must reimplement
the clamping table. Exceeding bounds may be harmless (EC ignores) or may cause
throttling/instability depending on model.

## Risks

- **Upstream driver coverage**: The upstream `uniwill-laptop` may not cover
  all features of all TUXEDO devices at launch. May need to contribute
  patches or keep temporary kernel modules.
- **Clevo gap**: Clevo support may not be in the upstream driver at all.
- **TDP control**: This is vendor-specific and may not have upstream sysfs.

## Deliverable

All WMI/ACPI/platform logic runs through either the upstream kernel driver's
sysfs or the Rust daemon. The `tuxedo_io` char device is retired.
