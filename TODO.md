# TODO: Feature Parity with tuxedo-drivers

Status key: DONE = implemented, PARTIAL = basic support, TODO = not started

## Fan Control

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| Basic PWM read/write | NB05 | DONE | Ranges + onereg variants |
| Basic PWM read/write | Uniwill | DONE | EC scale 0-200, no RPM |
| Basic PWM read/write | Tuxi | DONE | ACPI TFAN, native 0-255 |
| Basic PWM read/write | Clevo | DONE | WMI/ACPI DSM, FANINFO u32 |
| Fan curve engine | All | DONE | Generic FanBackend trait, 2s poll, 3°C hysteresis |
| Min speed enforcement | All | DONE | Configurable min_speed_percent (default 25%) |
| High-temp safety floor | NB05 | DONE | Top 2 registers enforce min duty |
| Fan table programming | Uniwill | TODO | Write 16-zone temp→duty tables to EC 0x0f00-0x0f5f |
| Fans-off detection | Uniwill | TODO | EC 0x078e bit 0x40 — some models allow 0% duty |
| Fan auto restore on crash | All | DONE | Systemd watchdog + startup/shutdown restore |

## Keyboard Backlight

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| ITE 8291 per-key RGB | USB HID | DONE | 6x21 grid, 4 products |
| ITE 8291 lightbar | USB HID | DONE | 3 products, 6 modes |
| ITE 8297 simple RGB | USB HID | DONE | 64-byte feature reports |
| ITE 829x per-key RGB | USB HID | DONE | 6x20 grid |
| Per-model color scaling | ITE 8291 | DONE | Model-specific R/G/B multipliers |
| Animation modes | ITE 8291 | PARTIAL | Mode enum defined, not all wired to D-Bus |
| NB05 white backlight | NB05 | DONE | 3 brightness levels via EC |
| Clevo backlight | Clevo | TODO | 4 types: none/fixed/3-zone/1-zone/per-key |
| Clevo backlight type detection | Clevo | TODO | CMD 0x0D byte[0x0f] or CMD 0x52 fallback |
| Clevo color scaling | Clevo | TODO | 1-zone: red*180/255, blue*200/255 |
| Clevo DMI quirks | Clevo | TODO | N14xWU/N13xWU force 5-step white |
| Uniwill backlight | Uniwill | TODO | 4 types via EC barebone ID (0x0740) + features (0x0766) |
| Uniwill EC LED control | Uniwill | TODO | EC 0x0769-076b (RGB), 0x0767 (mode), 0x1808 (brightness) |
| Uniwill DMI quirks | Uniwill | TODO | STELLARIS1XA05/STELLSL15I06 skip EC LED control |
| NB04 multicolor RGB | NB04 | TODO | WMI AB interface, brightness 0-10 |
| NB04 keyboard type detection | NB04 | TODO | NORMAL/PERKEY/4ZONE/WHITE via WMI AB |
| Suspend/resume restore | All | TODO | Save state, restore on resume |

## Power Profiles / TDP

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| NB04 profiles | NB04 | DONE | battery/balanced/performance via WMI BS 0x07 |
| NB05 profiles | NB05 | TODO | WMI GUID 99D89064, methods 1=write/2=read |
| NB05 profile persistence | NB05 | TODO | Timer-based to prevent firmware overrides |
| Uniwill profiles | Uniwill | TODO | EC 0x0727 custom profile mode |
| Uniwill TDP control | Uniwill | TODO | PL1=0x0783, PL2=0x0784, PL4=0x0785 |
| Uniwill TDP bounds | Uniwill | TODO | Per-device min/max for ~25 device variants |
| Clevo profiles | Clevo | TODO | CMD 0x79 sub-cmd 0x19 |
| D-Bus profile interface | All | PARTIAL | NB04 wired, others return stub |

## WMI Events / Hotkeys

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| Clevo events | Clevo | TODO | GUID ABBC0F6B, method 0x01 |
| Clevo input device | Clevo | TODO | Kbd brightness, touchpad, perf mode |
| Uniwill events | Uniwill | TODO | GUIDs ABBC0F70/71/72 |
| Uniwill i8042 filter | Uniwill | TODO | Touchpad toggle via scancode detection |
| NB05 events | NB05 | TODO | GUID 8FAFC061, touchpad/camera/Fn Lock/brightness |
| NB04 events | NB04 | TODO | GUID 96A786FA, 18 event codes |
| evdev input device | All | TODO | Kernel input device for key events |

## Charge Control / Battery

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| Clevo flexicharger | Clevo | TODO | Legacy (0x76/0x77) and CC4 (0x04 sub 0x1e/0x1f) |
| Clevo charge thresholds | Clevo | TODO | Start/stop percentages |
| Clevo battery hook | Clevo | TODO | acpi_battery_hook integration |
| Uniwill charging priority | Uniwill | TODO | EC 0x07cc |
| Uniwill charging profile | Uniwill | TODO | EC 0x07a6: high_capacity/balanced/stationary |
| Uniwill battery info | Uniwill | TODO | Cycle count (0x04A6), design/full capacity |

## Hardware Switches

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| Clevo webcam | Clevo | TODO | Read 0x06, write 0x22 |
| Clevo flightmode | Clevo | TODO | Read 0x07, write 0x20 |
| Clevo touchpad | Clevo | TODO | Read 0x09, write 0x2a |
| Uniwill USB powershare | Uniwill | TODO | EC 0x0767 |
| Uniwill AC auto boot | Uniwill | TODO | EC 0x0726 |
| NB05 toggles | NB05 | TODO | Touchpad, camera, Fn Lock via WMI events |
| Fn Lock | Clevo/Uniwill | TODO | Clevo: ACPI 0x04 sub 0x18/0x19, Uniwill: EC 0x074e |

## Sensors

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| CPU/GPU temp | NB05 | DONE | EC 0x0470 |
| Fan RPM | NB05 | DONE | EC 0x0298 (fan1), 0x0218 (fan2) |
| CPU/GPU temp | Uniwill | DONE | EC 0x043e, 0x044f |
| Fan RPM | Uniwill | N/A | No RPM registers on Uniwill EC |
| CPU/GPU temp, RPM | Tuxi | DONE | ACPI GTMP/GRPM |
| Clevo temp, RPM | Clevo | DONE | Parsed from FANINFO u32 |
| CPU/GPU temp, RPM | NB04 | DONE | WMI BS methods 0x04/0x06/0x02 |
| NB04 turbo/GPU freq | NB04 | TODO | Available in WMI BS but not exposed |

## NVIDIA GPU Power

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| cTGP/Dynamic Boost | Uniwill NB02 | TODO | EC 0x0743-0x0746, PCI 0x10de detection |
| cTGP offset | Uniwill NB02 | TODO | Default offset=0, sysfs attribute |
| TPP offset | Uniwill NB02 | TODO | Default=255 |
| Dynamic Boost offset | Uniwill NB02 | TODO | Default=25 |

## Uniwill Extended Features

| Feature | Status | Notes |
|---------|--------|-------|
| Lightbar control | TODO | EC 0x0749-074b (RGB), 0x0748 (animation mode) |
| Mini LED local dimming | TODO | WMI function 5, support flag at EC 0x0D4F |
| ROM ID | TODO | EC 0x0770-0x077f with correction registers |
| Custom profile mode flag | TODO | EC 0x0727 — required for TDP/fan changes on some devices |

## Suspend / Resume

| Feature | Platform | Status | Notes |
|---------|----------|--------|-------|
| Fan state restore | All | TODO | Re-apply fan mode after resume |
| Keyboard backlight restore | All | TODO | Re-apply brightness/color after resume |
| Profile restore | All | TODO | Re-apply active profile after resume |

## TCC Compatibility

| Feature | Status | Notes |
|---------|--------|-------|
| D-Bus API covering all ioctls | TODO | Map tuxedo_io ioctl commands to D-Bus methods |
| Introspection XML | TODO | For TCC migration |

## Infrastructure

| Feature | Status | Notes |
|---------|--------|-------|
| DKMS packaging | TODO | dkms.conf exists per shim, needs install target |
| udev rules (hidraw perms) | TODO | SUBSYSTEM=="hidraw", ATTRS{idVendor}=="048d" |
| Config hot-reload | TODO | watch::Sender exists but is dropped immediately |
| D-Bus config write-back | TODO | Allow TCC to push config changes via D-Bus |
| hwmon subsystem integration | TODO | Register as hwmon provider for standard tools |
| Hardware testing | TODO | Validate on each supported platform |

## Out of Scope (upstream separately)

| Module | Notes |
|--------|-------|
| stk8321 | I2C accelerometer — submit to IIO subsystem |
| gxtp7380 | ACPI touch panel uevent relay — submit to platform/x86 |
