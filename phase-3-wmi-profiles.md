# Phase 3: Fan Framework + Uniwill & Tuxi Platforms — DONE

**Goal**: Build the generic fan control framework (FanBackend trait, fan curve
engine, hwmon client) and self-contained platform adapters for Uniwill and Tuxi.
Each platform gets its own kernel shim — zero upstream kernel dependencies.

**Replaced**: `uniwill_wmi`, `tuxedo_keyboard`, `tuxedo_io` (Uniwill fan path),
`tuxedo_tuxi_fan_control`, `tuxi_acpi` (5 modules)

## Fan Control Framework

```
tuxedo-daemon/src/
├── fan_curve.rs     FanBackend trait, FanCurveEngine, interpolation, hysteresis
└── hwmon.rs         Generic hwmon sysfs client (find_by_name, temp/rpm/pwm R/W)
```

**FanBackend trait** — implemented by all platform adapters:
```rust
pub trait FanBackend: Send + Sync {
    fn read_temp(&self) -> io::Result<u8>;
    fn write_pwm(&self, fan_index: u8, pwm: u8) -> io::Result<()>;
    fn read_pwm(&self, fan_index: u8) -> io::Result<u8>;
    fn set_auto(&self, fan_index: u8) -> io::Result<()>;
    fn num_fans(&self) -> u8;
    fn read_fan_rpm(&self, fan_index: u8) -> io::Result<u16>;
}
```

**FanCurveEngine**: Polls temp, interpolates curve, applies hysteresis, writes PWM.
Runs as a tokio task. Config hot-reload via `watch::Receiver<FanConfig>`.

**Backend chain** in `main.rs`: NB05 → Uniwill → Tuxi → None

## Uniwill Platform (Self-Contained)

```
tuxedo-uw-fan-kmod/
├── tuxedo_uw_fan.c   ~200 LOC. ACPI ECRR/ECRW via \_SB.PCI0.SBRG.EC0 (LPCB fallback)
├── Makefile
└── dkms.conf

tuxedo-daemon/src/uniwill/
└── mod.rs            UniwillPlatform: FanBackend impl, PWM scaling (0-200 ↔ 0-255)
```

**Kernel shim sysfs** at `/sys/devices/platform/tuxedo-uw-fan/`:
- `fan0_pwm`, `fan1_pwm` (RW) — EC 0x1804 / 0x1809, range 0-200
- `fan_mode` (RW) — EC 0x0751 bit 7 (0=auto, 1=manual)
- `cpu_temp`, `gpu_temp` (RO) — EC 0x043e / 0x044f, degrees C
- `fan_count` (RO) — hardcoded "2"

**Key details**:
- PWM scaling: `ec_to_pwm(ec) = (ec*255+100)/200`, `pwm_to_ec(pwm) = (pwm*200+127)/255`
- No RPM registers in Uniwill EC — `read_fan_rpm()` returns `Ok(0)`
- Manual mode: set bit 7 of EC 0x0751 via ECRW
- ACPI path discovery: tries `EC0` under both `SBRG` and `LPCB`

## Tuxi Platform

```
tuxedo-tuxi-kmod/
├── tuxedo_tuxi.c     ~280 LOC. ACPI TFAN subdevice (HID TUXI0000)
├── Makefile
└── dkms.conf

tuxedo-daemon/src/tuxi/
└── mod.rs            TuxiPlatform: FanBackend impl, tenth-Kelvin conversion
```

**Kernel shim sysfs** at `/sys/devices/platform/tuxedo-tuxi/`:
- `fan_count` (RO) — TFAN.GCNT
- `fan_mode` (RW) — TFAN.GMOD / TFAN.SMOD (0=auto, 1=manual)
- `fan0_pwm`, `fan1_pwm` (RW) — TFAN.GSPD / TFAN.SSPD, range 0-255 (native)
- `fan0_temp`, `fan1_temp` (RO) — TFAN.GTMP, raw tenth-Kelvin
- `fan0_rpm`, `fan1_rpm` (RO) — TFAN.GRPM

**Key details**:
- Temperature conversion: `(tenth_kelvin - 2730) / 10` → degrees C
- PWM is native 0-255, no scaling needed (unlike Uniwill)
- `evaluate_int()` ACPI helper for calling methods with integer args/return
- On module unload: `restore_auto()` calls SMOD(0)

## Tests (14 total for this phase)

- Fan curve interpolation (at points, midpoint, below/above, empty)
- Hysteresis (update vs no-update)
- Percent-to-PWM conversion
- Hwmon sysfs client (find, read temp/rpm/pwm, write pwm, fan count)
- Uniwill PWM scaling round-trip, init, missing shim
- Tuxi tenth-Kelvin conversion, init + read, missing shim
