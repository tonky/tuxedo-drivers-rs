# Risks & Mitigations

## R1: Fan safety — userspace fan control

**Risk**: If the daemon crashes while fans are in manual mode, fans stay at
their last speed. Low speed during high CPU load → thermal issues.

**Impact**: Medium. EC firmware has its own thermal protection (emergency
shutdown at ~100°C), but prolonged operation near limits degrades hardware.

**Mitigation**:
- Systemd watchdog (`WatchdogSec=30`): restart daemon within seconds on crash
- On startup: always restore auto mode before taking control (clears stale state)
- On shutdown (SIGTERM/SIGINT): always write auto mode before exiting
- Kernel shims restore auto on module unload (`module_exit`)
- EC firmware thermal protection is the ultimate safety net
- High-temp safety floor on NB05: top 2 temp-band registers enforce minimum
  duty≥40, preventing full fan-off at high temperatures

## R2: EC register map accuracy

**Risk**: EC register addresses are reverse-engineered from vendor C drivers.
Registers may differ across firmware versions or new hardware revisions.

**Impact**: High for NB05 and Uniwill. Wrong register writes can affect thermals.

**Mitigation**:
- NB05: firmware version check gates RPM register writes (FW ≥ 9.10 skips them)
- Conservative defaults: unknown firmware → auto mode only
- All register addresses documented with source references to vendor code
- Shims only expose registers confirmed in vendor drivers; no speculative access
- Test on each hardware model before release

## R3: Kernel shim maintenance burden

**Risk**: Self-contained PoC means 3-4 kernel shims to maintain across kernel
versions. Out-of-tree modules may break with kernel API changes.

**Impact**: Medium. Breaks usually happen at major kernel releases (ACPI API,
platform_device, sysfs changes).

**Mitigation**:
- Shims are intentionally minimal (~150-280 LOC each): less surface area for breakage
- Use only stable kernel APIs (platform_driver, sysfs attrs, ACPI evaluate)
- DKMS handles rebuild on kernel upgrade
- Long-term: submit shims upstream to `platform/x86` where they'll be maintained
  by kernel developers

## R4: Limited sensor data on some platforms

**Risk**: Uniwill EC has no RPM registers — `read_fan_rpm()` returns 0.
NB04 has no direct fan PWM control. Sensor coverage varies by platform.

**Impact**: Low. Fan control still works (duty-based), just without RPM feedback.

**Mitigation**:
- `FanBackend` trait methods return `io::Result`, allowing graceful "not available"
- D-Bus clients handle 0 RPM as "not available" rather than "fans stopped"
- Fan curve engine operates on temperature + duty, not RPM
- Document per-platform capabilities clearly

## R5: No hardware testing yet

**Risk**: All code is written against vendor C driver analysis + sysfs simulation
in tests. No testing on actual TUXEDO hardware has occurred.

**Impact**: High. Register behavior, timing, and edge cases may differ from
what the C code suggests.

**Mitigation**:
- Unit tests with fake sysfs (tempdir-based) cover all adapter logic
- Kernel shims can be tested with `insmod` + manual `cat`/`echo` on target hardware
- Daemon gracefully falls back to None backend on non-matching hardware
- All platforms restore auto mode on any failure path
- Phase 4c explicitly includes end-to-end hardware testing

## R6: hidraw permissions and conflicts

**Risk**: Accessing `/dev/hidraw*` requires appropriate permissions. Other
software (OpenRGB, piper) may also claim ITE keyboard devices.

**Impact**: Low. Manageable with udev rules.

**Mitigation**:
- Ship udev rules: `SUBSYSTEM=="hidraw", ATTRS{idVendor}=="048d", MODE="0660", GROUP="tuxedo"`
- Use exclusive access when writing LED data
- Document conflict resolution with other RGB software

## R7: Clevo dual-transport complexity (Phase 4a)

**Risk**: Clevo requires negotiating ACPI DSM vs WMI transport. The 100ms
post-write delay ("no known ready flag") suggests timing-sensitive hardware.
Wrong transport or missing delay → silent failures or EC lockup.

**Impact**: Medium. Only affects Clevo-based models.

**Mitigation**:
- Probe both transports at module init, validate with cmd 0x52
- Enforce 100ms delay in kernel shim (not daemon — avoids userspace timing issues)
- Validate probe result: non-0xffffffff from GET_BIOS_FEATURES_1
- Fall back gracefully if neither transport works

## R8: TUXEDO Control Center compatibility

**Risk**: The existing GUI talks to `/dev/tuxedo_io` via ioctl. The new D-Bus
API is incompatible.

**Impact**: High for end users, but low for PoC scope.

**Mitigation**:
- Deferred to post-PoC. PoC validates the architecture, not the GUI integration.
- D-Bus introspection XML provides the migration path
- `tuxedo_io` ioctl mapping to D-Bus methods is straightforward

## Risk Summary

| Risk | Impact | Likelihood | Phase | Priority |
|------|--------|------------|-------|----------|
| R1: Fan safety | Medium | Low | All | P1 |
| R2: EC registers | High | Low | 2, 3 | P1 |
| R3: Shim maintenance | Medium | Medium | All | P2 |
| R4: Limited sensors | Low | Certain | 3 | P3 |
| R5: No HW testing | High | Certain | All | P1 |
| R6: hidraw perms | Low | Low | 1 | P3 |
| R7: Clevo transport | Medium | Medium | 4a | P2 |
| R8: CC compat | High | Certain | Post-PoC | P3 |
