# Risks & Mitigations

## R1: Upstream uniwill-laptop coverage gaps

**Risk**: The mainline `uniwill-laptop` driver (6.19+) may not support all
TUXEDO device features — especially newer models, TDP control, or charging
profiles.

**Impact**: High. Phase 3 depends heavily on upstream sysfs interfaces.

**Mitigation**:
- Track upstream patches: https://github.com/Wer-Wolf/uniwill-laptop
- Contribute missing features upstream (TUXEDO is already doing this)
- Keep a fallback: if a feature isn't upstream, the daemon can include a
  small optional kernel module for that specific WMI call
- Prioritize features that ARE upstream; defer others

## R2: Clevo devices not covered by upstream

**Risk**: `uniwill-laptop` targets Uniwill ODM hardware. Clevo uses different
WMI GUIDs (`ABBC0F6D-...`). There may not be an upstream Clevo driver.

**Impact**: Medium. Affects Clevo-based TUXEDO models.

**Mitigation**:
- Check for existing `clevo-laptop` or `clevo-wmi` upstream efforts
- If none exist, keep `clevo_wmi.c` as a separate small GPLv2 module
  and submit it upstream independently
- Or contribute Clevo support to `uniwill-laptop` if architecturally similar

## R3: EC register map accuracy

**Risk**: The EC register map is reverse-engineered from the current drivers.
Registers may differ across firmware versions or new NB05 models.

**Impact**: High for NB05 devices. Wrong register writes can affect thermals.

**Mitigation**:
- Firmware version check before writing (already done in C driver)
- Conservative defaults: if firmware version is unknown, use auto mode only
- Document all known register maps with firmware version ranges
- Test on each NB05 model before release

## R4: Safety — fan control in userspace

**Risk**: If the daemon crashes while fans are in manual mode, fans stay at
their last speed. If that speed is low during high CPU load, thermal issues.

**Impact**: Medium. EC firmware has its own thermal protection (emergency
shutdown), but prolonged operation near limits is bad for hardware.

**Mitigation**:
- Systemd watchdog: restart daemon within seconds on crash
- On daemon startup: check if manual mode was left active, restore auto
- On daemon shutdown (SIGTERM): always write auto mode before exiting
- EC firmware thermal protection is the ultimate safety net
- Note: the current kernel driver has a similar risk (manual mode persists
  if the module is unloaded), but a userspace daemon is more likely to crash
  than a kernel module, so this risk is slightly elevated in the new design.
  The systemd watchdog mitigation is essential, not optional.

## R5: hidraw permissions and conflicts

**Risk**: Accessing `/dev/hidraw*` requires appropriate permissions. Other
software (OpenRGB, piper) may also try to claim the device.

**Impact**: Low. Manageable with udev rules.

**Mitigation**:
- Ship udev rules: `SUBSYSTEM=="hidraw", ATTRS{idVendor}=="048d", MODE="0660", GROUP="tuxedo"`
- Use exclusive access when writing LED data
- Document conflict resolution with other RGB software

## R6: TUXEDO Control Center compatibility

**Risk**: The existing GUI (TUXEDO Control Center) talks to `/dev/tuxedo_io`
via ioctl. Switching to D-Bus breaks the GUI.

**Impact**: High for end users.

**Mitigation**:
- Coordinate with TUXEDO CC team early
- Provide D-Bus client libraries in their language (TypeScript/Python)
- Consider a compat shim (FUSE /dev/tuxedo_io → D-Bus) as temporary bridge
- Phase the transition: run both old and new in parallel (phase 4.9)

## R7: Kernel version requirements

**Risk**: Users on older kernels (< 6.19) won't have `uniwill-laptop` in
mainline. They'll need the old kernel modules.

**Impact**: Medium. Affects users on LTS distros.

**Mitigation**:
- For kernels < 6.19: keep the old tuxedo-drivers DKMS package as fallback
- The daemon's hwmon/sysfs clients work with any kernel that has the
  appropriate sysfs files — doesn't matter if the driver is upstream or DKMS
- Document minimum kernel version requirements per feature

## R8: I2C accelerometer permission

**Risk**: Accessing `/dev/i2c-*` for the STK8321 accelerometer requires
`CAP_SYS_ADMIN` or membership in the `i2c` group on most distros.

**Impact**: Low. Accelerometer is a minor feature.

**Mitigation**:
- Ship udev rule for the specific I2C bus
- Or use the existing in-kernel `stk8321` IIO driver (if it gets upstream)
  and read from IIO sysfs instead
- Make accelerometer support optional — daemon works without it

## R9: WMI GUID conflict during transition

**Risk**: On kernels >= 6.19, both old tuxedo-drivers modules and upstream
`uniwill-laptop` may be present. They register the same WMI GUIDs — only
one can bind, the other fails silently.

**Impact**: High. Fan/thermal control silently stops working.

**Mitigation**:
- Ship modprobe blacklist for old modules when upstream is available
- Daemon detects which driver is active at startup (see Phase 3 conflict
  resolution strategy)
- Package-level `Conflicts:` prevents coinstallation

## Risk Summary

| Risk | Impact | Likelihood | Priority |
|------|--------|------------|----------|
| R1: Upstream gaps | High | Medium | P1 |
| R2: Clevo gap | Medium | High | P1 |
| R3: EC register map | High | Low | P2 |
| R4: Fan safety | Medium | Low | P2 |
| R5: hidraw perms | Low | Low | P3 |
| R6: CC compat | High | Certain | P1 |
| R7: Kernel version | Medium | Medium | P2 |
| R8: I2C perms | Low | Low | P3 |
| R9: WMI conflict | High | High | P1 |
