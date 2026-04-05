# Phase 2: NB05 EC Kernel Shim + Fan/Sensor/Backlight — DONE

**Goal**: Replace 6 NB05 kernel modules with 1 minimal kernel shim + userspace control.

**Replaced**: `tuxedo_nb05_ec`, `tuxedo_nb05_fan_control`, `tuxedo_nb05_sensors`,
`tuxedo_nb05_kbd_backlight`, `tuxedo_nb05_keyboard`, `tuxedo_nb05_power_profiles`
(6 modules, ~1.8K LOC)

## Kernel Shim

```
tuxedo-ec-kmod/
├── tuxedo_ec.c      ~180 LOC. EC port I/O (0x4e/0x4f), sysfs binary attr
├── Makefile         Standard out-of-tree build
└── dkms.conf        DKMS packaging
```

- Exposes `/sys/devices/platform/tuxedo-ec/ec_ram` (mode 0600)
- Userspace uses `pread(2)`/`pwrite(2)` for atomic single-byte EC access
- Kernel mutex serializes all EC port I/O
- DMI match: `board_vendor == "NB05"`

## Rust Daemon

```
tuxedo-daemon/src/
├── ec.rs             EcClient: pread/pwrite wrapper over sysfs ec_ram
└── nb05/
    ├── mod.rs        Nb05Platform: init, shutdown, FanBackend impl
    ├── fan.rs        Fan control: deadband, high-temp floor, PWM/duty/RPM
    ├── sensors.rs    EC register reads for CPU/GPU temp, fan RPM
    └── kbd_backlight.rs  EC-based keyboard brightness
```

## Key Implementation Details

- **Two hardware variants**: Multi-register (Pulse, 2 fans) vs single-register
  (InfinityFlex, 1 fan, 0xaa sentinel)
- **Firmware version gate**: `write_rpm = true` only if FW < 9.10 AND not onereg
- **Deadband**: 0 → off, ≤12.5% → off, <25% → clamp to 25%, else pass-through
- **High-temp safety floor**: Top 2 of 9 temp-band registers get `max(value, 40)`
  for duty, `max(value, 15)` for RPM
- **PWM conversion**: `duty_to_pwm = (duty*255+50)/100`, `pwm_to_duty = (pwm*100+127)/255`
- **NB05 power profiles**: Use WMI (GUID `99D89064`), NOT EC registers — deferred

## Tests

- PWM/duty round-trip conversion
- Deadband logic (off, snap-up, pass-through)
- RPM conversion
- should_write_rpm firmware gate
