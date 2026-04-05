# Phase 2: NB05 EC Kernel Shim + Fan/Sensor Daemon

**Goal**: Replace the 6 NB05 kernel modules with 1 minimal kernel module
(tuxedo-ec) + userspace fan/sensor control in the Rust daemon.

**Depends on**: Phase 0 (daemon skeleton).

**Replaces**: `tuxedo_nb05_ec`, `tuxedo_nb05_fan_control`, `tuxedo_nb05_sensors`,
`tuxedo_nb05_kbd_backlight`, `tuxedo_nb05_keyboard`, `tuxedo_nb05_power_profiles`
(6 kernel modules, ~1.8K LOC)

## Why This Needs a Kernel Module

The NB05 EC communicates via direct I/O port access (`outb`/`inb` on ports
0x4e/0x4f). This requires ring-0 privileges and cannot be safely done from
userspace. The EC uses an indirect addressing scheme:

```c
// Write address high byte, then low byte, then read/write data
outb(I2EC_REG_ADDR, 0x4e);   // select address register
outb(I2EC_ADDR_HIGH, 0x4f);  // write high address
outb(I2EC_REG_ADDR, 0x4e);
outb(addr_high, 0x4f);
// ... (see tuxedo-drivers/src/tuxedo_nb05/tuxedo_nb05_ec.c:47-96)
```

## Tasks

### 2.1 — tuxedo-ec kernel module (C, GPLv2)

Create `tuxedo-ec-kmod/tuxedo_ec.c` — a minimal module that ONLY does:

1. DMI matching for NB05 devices (3 models currently)
2. Mutex-protected EC RAM read/write via I/O ports
3. Exposes EC RAM access to userspace via one of:

**Option A: sysfs binary attribute with pread/pwrite** (preferred for upstream)
```
/sys/devices/platform/tuxedo-ec/ec_ram
```
Userspace uses `pread(fd, &data, 1, addr)` / `pwrite(fd, &data, 1, addr)` to
access EC RAM. `pread`/`pwrite` are **single syscalls** that combine seek+read
into an atomic operation — unlike separate `lseek` + `read` which is two
syscalls with no atomicity guarantee (another process could seek in between).

**Option B: char device with ioctl**
```
/dev/tuxedo-ec
ioctl(fd, TUXEDO_EC_READ, &{addr, data})
ioctl(fd, TUXEDO_EC_WRITE, &{addr, data})
```
More familiar but harder to upstream.

**Recommendation**: Option A. Binary sysfs attributes are well-understood by
upstream reviewers. The `ec_ram` file should be restricted to root (mode 0600).
The Rust `EcClient` must use `pread(2)`/`pwrite(2)` — NOT `seek()` + `read()`.

Note: the kernel module's internal mutex ensures EC port I/O is serialized
regardless of the userspace access pattern, but `pread`/`pwrite` avoids the
need for userspace-side locking when multiple threads access different registers.

Target size: **~150 LOC** (vs current 220 LOC in tuxedo_nb05_ec.c, and the
module only exports raw read/write — no policy).

### 2.2 — EC register map

Document the NB05 EC RAM register map extracted from the current drivers.
This is critical knowledge that lives only in the C source today:

```
Fan Control:
  0x02c0       fan1 manual enable (0x01 = manual)
  0x02c1-0x02c9  fan1 duty cycle per temp range
  0x0240       fan2 manual enable
  0x0241-0x0249  fan2 duty cycle per temp range
  0x02d0-0x02d8  fan1 RPM targets per temp range
  0x0250-0x0258  fan2 RPM targets per temp range

  One-reg variant (InfinityFlex):
  0x1809       fan1 duty (single register)
  0x02f1       fan1 manual enable (0xaa = manual)

Sensors (from tuxedo_nb05_sensors.c):
  (need to extract from sensors driver)

Keyboard Backlight (from tuxedo_nb05_kbd_backlight.c):
  (need to extract from kbd backlight driver)

Power Profiles (from tuxedo_nb05_power_profiles.c):
  (need to extract from profiles driver)

Firmware Version:
  0x0400       major version
  0x0401       minor version
```

### 2.3 — Rust EC client library

Create `tuxedo-daemon/src/ec.rs`:

```rust
pub struct EcClient {
    sysfs_path: PathBuf,  // /sys/devices/platform/tuxedo-ec/ec_ram
    file: File,
}

impl EcClient {
    pub fn read(&self, addr: u16) -> io::Result<u8> {
        let mut buf = [0u8; 1];
        nix::sys::uio::pread(&self.file, &mut buf, addr as i64)?;
        Ok(buf[0])
    }
    pub fn write(&self, addr: u16, data: u8) -> io::Result<()> {
        nix::sys::uio::pwrite(&self.file, &[data], addr as i64)?;
        Ok(())
    }
}
```

### 2.4 — Fan control in userspace

Port fan control logic from `tuxedo_nb05_fan_control.c` to Rust:

```rust
pub struct NB05FanController {
    ec: EcClient,
    device: NB05DeviceData,  // number_fans, fanctl_onereg, write_rpm
}

impl NB05FanController {
    pub fn set_fan_pwm(&self, fan: u8, pwm: u8) -> Result<()>;
    pub fn get_fan_pwm(&self, fan: u8) -> Result<u8>;
    pub fn set_auto_mode(&self, fan: u8, enable: bool) -> Result<()>;
    pub fn is_auto_mode(&self, fan: u8) -> Result<bool>;
}
```

**Critical implementation details** (from `tuxedo_nb05_fan_control.c`):

#### 2.4.1 — Firmware version gate (`write_rpm`)

At daemon startup, read EC firmware version from registers 0x0400/0x0401.
Set `write_rpm = true` ONLY if:
- firmware version < 9.10, AND
- `fanctl_onereg == false`

When `write_rpm` is false, the daemon must ONLY write duty-cycle registers
(0x02c1-0x02c9 / 0x0241-0x0249). Writing RPM registers on firmware >= 9.10
causes undefined behavior. When `write_rpm` is true, write BOTH duty and RPM
register sets.

#### 2.4.2 — Deadband logic (NOT a simple clamp)

`FAN_ON_MIN_SPEED_PERCENT = 25`. The clamping is a **deadband** to prevent
fan motor stall:

```
if value == 0:
    pass through as 0 (fan off)
elif value <= 12.5%:     (i.e. <= FAN_ON_MIN_SPEED / 2)
    clamp DOWN to 0 (fan off — too low to spin reliably)
elif value < 25%:
    clamp UP to 25% (minimum reliable spin speed)
else:
    pass through as-is
```

A naive `max(value, 25%)` would prevent fans from ever turning off. A naive
pass-through would cause fans to stall and buzz at low duty cycles.

#### 2.4.3 — High-temperature safety floor (top 2 registers only)

The NB05 fan uses 9 temperature-indexed registers per fan per direction
(duty and RPM). The safety floor applies ONLY to the top 2 registers
(the two highest temperature bands):

```
Registers 0..6 (temp bands 0-6): write user's clamped value
Registers 7..8 (temp bands 7-8): write MAX(user_value, floor)

Floor constants:
  FAN_SET_DUTY_HIGHTEMP = 40   (for duty registers)
  FAN_SET_RPM_HIGHTEMP  = 15   (for RPM registers, raw EC units)
```

Fan 1 duty registers: 0x02c1-0x02c9 (floor applies to 0x02c8, 0x02c9)
Fan 1 RPM registers:  0x02d0-0x02d8 (floor applies to 0x02d7, 0x02d8)
Fan 2 duty registers: 0x0241-0x0249 (floor applies to 0x0248, 0x0249)
Fan 2 RPM registers:  0x0250-0x0258 (floor applies to 0x0257, 0x0258)

Getting this wrong either makes fans always loud (floor too broad) or
removes thermal safety (floor not applied).

#### 2.4.4 — Two hardware variants

**Multi-register variant** (Pulse models, `fanctl_onereg=false`):
- Manual enable: write `0x01` to 0x02c0 (fan1) / 0x0240 (fan2)
- Auto mode: write `0x00` to same registers
- 9 duty registers + 9 RPM registers per fan

**Single-register variant** (InfinityFlex, `fanctl_onereg=true`):
- Manual enable: write `0xaa` to 0x02f1 (NOT `0x01`)
- Auto mode: write `0x00` to 0x02f1
- Single duty register at 0x1809 (fan1 only, 1-fan device)
- No RPM register writes

Mixing up the sentinel values (`0x01` vs `0xaa`) produces silent failures
where the EC ignores the mode switch.

#### 2.4.5 — PWM conversion

```
duty_to_pwm(duty) = (duty * 255 + 100/2) / 100   // 0-100% → 0-255
pwm_to_duty(pwm)  = (pwm * 100 + 255/2) / 255    // 0-255 → 0-100%
```

### 2.5 — Sensor reading in userspace

Port temperature/fan RPM reading from `tuxedo_nb05_sensors.c`:
- Read CPU/GPU temperatures from EC registers
- Read fan RPM from EC registers
- Expose via D-Bus `com.tuxedo.Daemon.Fan.GetTemperature()`

### 2.6 — Keyboard backlight via EC

Port `tuxedo_nb05_kbd_backlight.c`:
- Brightness control via EC register writes
- Expose via D-Bus `com.tuxedo.Daemon.Keyboard.SetBrightness()`

### 2.7 — Power profiles via EC

Port `tuxedo_nb05_power_profiles.c`:
- Performance mode switching via EC registers
- Expose via D-Bus `com.tuxedo.Daemon.Profile`

### 2.8 — DKMS packaging for tuxedo-ec

```ini
# tuxedo-ec-kmod/dkms.conf
PACKAGE_NAME="tuxedo-ec"
PACKAGE_VERSION="1.0.0"
BUILT_MODULE_NAME[0]="tuxedo_ec"
DEST_MODULE_LOCATION[0]="/kernel/drivers/platform/x86/"
AUTOINSTALL="yes"
```

## Safety Considerations

- The EC has a high-temp safety floor: never write a fan speed that could
  cause thermal shutdown. The min-speed enforcement from the C driver MUST
  be preserved in Rust.
- EC access must be serialized (the kernel module handles the mutex).
- The sysfs file should be mode 0600 (root only).

## Testing Strategy

1. Build tuxedo-ec module, load it, verify `/sys/devices/platform/tuxedo-ec/ec_ram` exists
2. Test read: `dd if=ec_ram bs=1 skip=$((0x0400)) count=1 | xxd` (firmware version)
3. Run daemon, verify fan control works via D-Bus
4. Stress test: rapid read/write to verify mutex works
5. Thermal safety: verify min-speed enforcement

## Deliverable

- `tuxedo-ec` kernel module (~150 LOC, GPLv2, upstreamable)
- Daemon controls NB05 fans, sensors, backlight, profiles via EC sysfs
- 6 kernel modules replaced by 1
