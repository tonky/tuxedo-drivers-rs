# Hardware Testing Reference

## Platform Detection

Run on the target TUXEDO laptop to identify its platform:

```bash
# DMI fields used by the daemon
cat /sys/class/dmi/id/sys_vendor
cat /sys/class/dmi/id/board_vendor
cat /sys/class/dmi/id/board_name
cat /sys/class/dmi/id/product_sku
cat /sys/class/dmi/id/chassis_vendor

# WMI GUIDs — presence determines platform
ls /sys/bus/wmi/devices/ | grep -iE '80C9|ABBC|1F17'
# 80C9BAA6 → NB04
# ABBC0F6D + ABBC0F6E → Uniwill
# ABBC0F6D alone → Clevo (but ACPI check is authoritative)

# Clevo ACPI device
ls /sys/bus/acpi/devices/ | grep CLV
# CLV0001:00 → Clevo

# NB05 EC
ls /sys/devices/platform/tuxedo-ec/ec_ram 2>/dev/null && echo "NB05 EC present"
```

### Detection Logic (dmi.rs)

1. `board_vendor == "NB05"` → **NB05**
2. WMI GUID `80C9BAA6` present → **NB04**
3. `product_sku` starts with `"TUXI"` → **Tuxi**
4. ACPI device `CLV0001:00` exists → **Clevo**
5. WMI GUIDs `ABBC0F6D` + `ABBC0F6E` both present → **Uniwill**
6. Board name matches Clevo prefix (N85, N87, N95, P65, P775, W6) → **Clevo** (heuristic)
7. Fallback → **Uniwill** (most common for modern TUXEDO devices)

## Known Devices

| Model | Platform | product_sku | board_vendor | Notes |
|-------|----------|-------------|--------------|-------|
| Pulse 14 Gen3 | NB05 | PULSE1403 | NB05 | 2 fans, ranges variant |
| Pulse 14 Gen4 | NB05 | PULSE1404 | NB05 | 2 fans, ranges variant |
| InfinityFlex 14 Gen1 | NB05 | IFLX14I01 | NB05 | 1 fan, onereg variant |
| InfinityBook 16 Gen8 | Uniwill | IBP16I08MK2? | ? | Board name likely PH4PRX1_PH6PRX1. Verify on device. |

## Kernel Shim Testing

### Build a shim

```bash
cd tuxedo-clevo-kmod   # or whichever platform
make -C /lib/modules/$(uname -r)/build M=$(pwd) modules
```

### Load and verify

```bash
sudo insmod tuxedo_clevo.ko   # or tuxedo_tuxi.ko, etc.
ls /sys/devices/platform/tuxedo-clevo/   # check sysfs appeared
dmesg | tail -20   # check for probe messages or errors
```

### Read sensors (platform-specific)

```bash
# NB05 (via tuxedo-ec)
cat /sys/devices/platform/tuxedo-ec/ec_ram   # raw EC access

# Uniwill (via tuxedo-uw-fan)
cat /sys/devices/platform/tuxedo-uw-fan/cpu_temp
cat /sys/devices/platform/tuxedo-uw-fan/gpu_temp
cat /sys/devices/platform/tuxedo-uw-fan/fan0_pwm
cat /sys/devices/platform/tuxedo-uw-fan/fan1_pwm
cat /sys/devices/platform/tuxedo-uw-fan/fan_count

# Tuxi (via tuxedo-tuxi)
cat /sys/devices/platform/tuxedo-tuxi/fan0_temp   # tenth-Kelvin
cat /sys/devices/platform/tuxedo-tuxi/fan0_rpm
cat /sys/devices/platform/tuxedo-tuxi/fan_count

# Clevo (via tuxedo-clevo)
cat /sys/devices/platform/tuxedo-clevo/fan0_info   # raw u32: duty|temp<<8|rpm<<16
cat /sys/devices/platform/tuxedo-clevo/fan1_info
cat /sys/devices/platform/tuxedo-clevo/fan2_info

# NB04 (via tuxedo-nb04)
cat /sys/devices/platform/tuxedo-nb04/cpu_temp
cat /sys/devices/platform/tuxedo-nb04/gpu_temp
cat /sys/devices/platform/tuxedo-nb04/fan0_rpm
cat /sys/devices/platform/tuxedo-nb04/fan1_rpm
cat /sys/devices/platform/tuxedo-nb04/power_profile
```

### Unload

```bash
sudo rmmod tuxedo_clevo   # restores fan auto mode on exit
```

## Daemon Testing

```bash
# Build
cargo build

# Run with debug logging (no systemd)
RUST_LOG=debug cargo run

# Check D-Bus
busctl tree com.tuxedo.Daemon
busctl introspect com.tuxedo.Daemon /com/tuxedo/Daemon
busctl get-property com.tuxedo.Daemon /com/tuxedo/Daemon com.tuxedo.Daemon.Device Platform
```

## Safety Notes

- Always test shim `insmod` / `rmmod` before running the daemon
- The daemon restores fan auto mode on startup, shutdown, and SIGTERM
- Kernel shims restore auto mode on `rmmod`
- EC firmware has thermal protection (~100C emergency shutdown) as safety net
- Start with `FanMode::Auto` in config (the default) — this lets hardware control fans
- Only switch to `CustomCurve` after verifying sensor reads return sane values
