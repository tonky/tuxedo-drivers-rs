# Phase 4: Full Integration & Cleanup

**Goal**: Polish, package, test, and ship the complete hybrid system.

**Depends on**: Phases 0-3.

## Tasks

### 4.1 — End-to-end testing on each platform

Test matrix:

| Platform   | Example Device          | Fan | LED | Profile | Sensors | EC |
|-----------|-------------------------|-----|-----|---------|---------|-----|
| NB05      | Pulse 14 Gen3/Gen4      | EC  | EC  | EC      | EC      | Yes |
| NB05      | InfinityFlex 14 Gen1    | EC  | EC  | EC      | EC      | Yes |
| NB04      | (various)               | WMI | WMI | WMI     | WMI     | No  |
| Uniwill   | (various)               | WMI | WMI | WMI     | WMI     | No  |
| Clevo     | (various)               | WMI | WMI | WMI     | WMI     | No  |
| ITE HID   | (Pulse/Polaris w/ USB)  | —   | HID | —       | —       | No  |
| Tuxi      | (TUXEDO Tuxi models)    | ACPI| —   | —       | —       | No  |

### 4.2 — Packaging

**Daemon (Rust binary)**:
```
- Debian: tuxedo-daemon (deb)
- RPM: tuxedo-daemon (rpm)
- Arch: tuxedo-daemon (AUR)
```

**Kernel module (tuxedo-ec only, for NB05 devices)**:
```
- DKMS package: tuxedo-ec-dkms
- Only needed for NB05 devices (Pulse, InfinityFlex)
```

**Transition package**:
```
- tuxedo-drivers → depends on tuxedo-daemon + tuxedo-ec-dkms
- Conflicts: old tuxedo-drivers-dkms
- Provides migration script to convert old config
```

### 4.3 — Config migration

Write a migration tool:
```
tuxedo-daemon migrate --from-tuxedo-io
```

Reads current state from `/dev/tuxedo_io` (if old driver loaded) and writes
equivalent `config.toml` for the new daemon.

### 4.4 — TUXEDO Control Center compatibility

TUXEDO Control Center (GUI) currently talks to `/dev/tuxedo_io` via ioctl.
It needs to be updated to use D-Bus instead. Provide:

- D-Bus introspection XML for the GUI team
- A reference Python/TypeScript client library
- Documentation of the ioctl → D-Bus mapping

### 4.5 — CLI tool

Optional: `tuxedo-ctl` CLI for quick access:

```bash
tuxedo-ctl fan status          # Show fan speeds and temps
tuxedo-ctl fan set 0 80        # Set fan 0 to 80% (manual)
tuxedo-ctl fan auto            # Return to auto mode
tuxedo-ctl kbd brightness 50   # Set keyboard brightness
tuxedo-ctl kbd color ff0000    # Set keyboard color (red)
tuxedo-ctl profile performance # Set performance profile
tuxedo-ctl info                # Show device info
```

Implemented as a thin D-Bus client.

### 4.6 — Monitoring and safety

- Watchdog: if daemon crashes, fan control returns to EC auto mode
  (the kernel/EC firmware handles this natively — manual mode is sticky
  but EC has thermal protection)
- Systemd watchdog integration (`sd_notify`, `WatchdogSec=`)
- Graceful shutdown: restore auto fan mode on daemon stop

### 4.7 — Documentation

- README with architecture overview
- Per-platform hardware notes
- D-Bus API reference (generated from introspection XML)
- Contributing guide
- EC register map document (from phase 2)

### 4.8 — Upstream tuxedo-ec submission

Once the module is stable:
1. Submit to `platform/x86` maintainers
2. Follow kernel coding style, checkpatch.pl clean
3. Add to MAINTAINERS file
4. Target kernel 6.21+ (assuming 6.19 has uniwill-laptop)

### 4.9 — Deprecation path for old tuxedo-drivers

```
v5.0:  Ship tuxedo-daemon alongside old kernel modules (both work)
v5.1:  Default to daemon, kernel modules optional
v6.0:  Remove old kernel modules from package
```

## Deliverable

Complete, shippable replacement for tuxedo-drivers:
- 1 Rust daemon (systemd service, D-Bus API)
- 1 kernel module (tuxedo-ec, NB05 only, upstreamable)
- CLI tool
- Packages for Debian/RPM/Arch
- Documentation
