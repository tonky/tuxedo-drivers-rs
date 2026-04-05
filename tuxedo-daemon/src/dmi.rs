use std::fs;
use std::path::Path;
use tracing::debug;

// WMI GUIDs from the C drivers — used for platform detection.
const NB04_WMI_AB_GUID: &str = "80C9BAA6-AC48-4538-9234-9F81A55E7C85";
const UNIWILL_WMI_MGMT_GUID_BA: &str = "ABBC0F6D-8EA1-11D1-00A0-C90629100000";
const UNIWILL_WMI_MGMT_GUID_BB: &str = "ABBC0F6E-8EA1-11D1-00A0-C90629100000";
const CLEVO_WMI_METHOD_GUID: &str = "ABBC0F6D-8EA1-11D1-00A0-C90629100000";
const CLEVO_ACPI_HID: &str = "CLV0001";

/// Hardware platform type, determines which control path to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// NB05-based devices (Pulse 14 Gen3/Gen4, InfinityFlex 14).
    /// Controlled via EC port I/O through tuxedo-ec kernel module.
    Nb05,
    /// NB04-based devices. Controlled via WMI.
    Nb04,
    /// Uniwill-based devices. Controlled via upstream uniwill-laptop sysfs/hwmon.
    Uniwill,
    /// Clevo-based devices. Controlled via WMI (may need separate kernel module).
    Clevo,
    /// TUXEDO Tuxi devices. Fan control via ACPI sysfs.
    Tuxi,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Nb05 => "nb05",
            Platform::Nb04 => "nb04",
            Platform::Uniwill => "uniwill",
            Platform::Clevo => "clevo",
            Platform::Tuxi => "tuxi",
        }
    }
}

/// Keyboard backlight type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardType {
    /// ITE 8291 per-key RGB via USB HID (hidraw).
    ItePerKey,
    /// ITE 8291 zone-based RGB via USB HID (hidraw).
    IteZones,
    /// ITE 8291 lightbar variant via USB HID (hidraw).
    IteLightbar,
    /// ITE 8297 via USB HID (hidraw).
    Ite8297,
    /// ITE 829x via USB HID (hidraw).
    Ite829x,
    /// WMI-based keyboard backlight (Uniwill/Clevo/NB04).
    Wmi,
    /// EC-based keyboard backlight (NB05).
    Ec,
    /// No controllable backlight.
    None,
}

/// NB05-specific device configuration.
#[derive(Debug, Clone)]
pub struct Nb05Data {
    /// Number of fans (1 for InfinityFlex, 2 for Pulse).
    pub num_fans: u8,
    /// Single-register fan control variant (InfinityFlex).
    pub fanctl_onereg: bool,
}

/// Detected TUXEDO device.
#[derive(Debug, Clone)]
pub struct TuxedoDevice {
    pub name: &'static str,
    pub platform: Platform,
    pub product_sku: String,
    pub keyboard_type: KeyboardType,
    pub nb05_data: Option<Nb05Data>,
}

/// Read a single DMI sysfs field, trimmed.
pub fn read_dmi_field(field: &str) -> Option<String> {
    let path = format!("/sys/class/dmi/id/{field}");
    fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Check if any of the vendor fields contain "TUXEDO".
fn is_tuxedo_vendor() -> bool {
    for field in ["sys_vendor", "board_vendor", "chassis_vendor"] {
        if let Some(val) = read_dmi_field(field) {
            if val.contains("TUXEDO") {
                return true;
            }
        }
    }
    false
}

/// Detect the connected TUXEDO device by reading DMI sysfs fields and WMI GUIDs.
/// Returns `None` if this is not a recognized TUXEDO device.
pub fn detect_device() -> Option<TuxedoDevice> {
    if !is_tuxedo_vendor() {
        debug!("not a TUXEDO device (no TUXEDO vendor string in DMI)");
        return None;
    }

    let product_sku = read_dmi_field("product_sku").unwrap_or_default();
    let board_vendor = read_dmi_field("board_vendor").unwrap_or_default();
    let board_name = read_dmi_field("board_name").unwrap_or_default();

    debug!(product_sku, board_vendor, board_name, "DMI fields read");

    // NB05 devices: board_vendor == "NB05" (DMI match, same as C driver)
    if board_vendor == "NB05" {
        return Some(match_nb05(&product_sku));
    }

    // NB04 devices: detected by WMI GUID presence (authoritative)
    if wmi_guid_present(NB04_WMI_AB_GUID) {
        debug!("NB04 WMI GUID detected");
        return Some(TuxedoDevice {
            name: "TUXEDO NB04 Device",
            platform: Platform::Nb04,
            product_sku: product_sku.clone(),
            keyboard_type: KeyboardType::Wmi,
            nb05_data: None,
        });
    }

    // Tuxi devices: SKU-based detection
    if product_sku.starts_with("TUXI") {
        return Some(TuxedoDevice {
            name: "TUXEDO Tuxi",
            platform: Platform::Tuxi,
            product_sku: product_sku.clone(),
            keyboard_type: KeyboardType::None,
            nb05_data: None,
        });
    }

    // Clevo detection: ACPI HID CLV0001 is authoritative.
    // WMI GUID ABBC0F6D overlaps with Uniwill so we check ACPI first.
    if has_clevo_acpi() {
        debug!("Clevo ACPI HID detected");
        return Some(TuxedoDevice {
            name: "TUXEDO Clevo Device",
            platform: Platform::Clevo,
            product_sku: product_sku.clone(),
            keyboard_type: KeyboardType::Wmi,
            nb05_data: None,
        });
    }

    // Uniwill detection: presence of multiple WMI GUIDs (ABBC0F6D + ABBC0F6E)
    // We check both BA and BB to distinguish from Clevo (which only has BA).
    if wmi_guid_present(UNIWILL_WMI_MGMT_GUID_BA) && wmi_guid_present(UNIWILL_WMI_MGMT_GUID_BB) {
        debug!("Uniwill WMI GUIDs detected");
        return Some(TuxedoDevice {
            name: "TUXEDO Uniwill Device",
            platform: Platform::Uniwill,
            product_sku: product_sku.clone(),
            keyboard_type: KeyboardType::Wmi,
            nb05_data: None,
        });
    }

    // Fallback: if WMI detection didn't match but we're on TUXEDO hardware,
    // check Clevo board name heuristic (WMI device nodes may not exist yet)
    if is_clevo_board(&board_name) {
        debug!("Clevo detected via board name heuristic");
        return Some(TuxedoDevice {
            name: "TUXEDO Clevo Device",
            platform: Platform::Clevo,
            product_sku: product_sku.clone(),
            keyboard_type: KeyboardType::Wmi,
            nb05_data: None,
        });
    }

    // Default: assume Uniwill platform (most common for modern TUXEDO devices)
    Some(TuxedoDevice {
        name: "TUXEDO Uniwill Device",
        platform: Platform::Uniwill,
        product_sku: product_sku.clone(),
        keyboard_type: KeyboardType::Wmi,
        nb05_data: None,
    })
}

fn match_nb05(product_sku: &str) -> TuxedoDevice {
    match product_sku {
        "PULSE1403" => TuxedoDevice {
            name: "TUXEDO Pulse 14 Gen3",
            platform: Platform::Nb05,
            product_sku: product_sku.to_string(),
            keyboard_type: KeyboardType::Ec,
            nb05_data: Some(Nb05Data {
                num_fans: 2,
                fanctl_onereg: false,
            }),
        },
        "PULSE1404" => TuxedoDevice {
            name: "TUXEDO Pulse 14 Gen4",
            platform: Platform::Nb05,
            product_sku: product_sku.to_string(),
            keyboard_type: KeyboardType::Ec,
            nb05_data: Some(Nb05Data {
                num_fans: 2,
                fanctl_onereg: false,
            }),
        },
        "IFLX14I01" => TuxedoDevice {
            name: "TUXEDO InfinityFlex 14 Gen1",
            platform: Platform::Nb05,
            product_sku: product_sku.to_string(),
            keyboard_type: KeyboardType::Ec,
            nb05_data: Some(Nb05Data {
                num_fans: 1,
                fanctl_onereg: true,
            }),
        },
        _ => TuxedoDevice {
            name: "TUXEDO NB05 Device (unknown model)",
            platform: Platform::Nb05,
            product_sku: product_sku.to_string(),
            keyboard_type: KeyboardType::Ec,
            nb05_data: Some(Nb05Data {
                num_fans: 2,
                fanctl_onereg: false,
            }),
        },
    }
}

/// Known Clevo board name prefixes from the C driver.
/// Fallback heuristic — WMI/ACPI detection is authoritative.
fn is_clevo_board(board_name: &str) -> bool {
    let clevo_prefixes = ["N85", "N87", "N95", "N96", "P65", "P775", "W6"];
    clevo_prefixes.iter().any(|p| board_name.starts_with(p))
}

/// Check if a WMI GUID is registered in /sys/bus/wmi/devices/.
/// WMI device nodes are named like "{GUID}-0" (case-insensitive).
fn wmi_guid_present(guid: &str) -> bool {
    // WMI sysfs entries use uppercase GUIDs
    let upper = guid.to_uppercase();
    // Check common suffixes: -0, -1, etc.
    for suffix in 0..4 {
        let path = format!("/sys/bus/wmi/devices/{upper}-{suffix}");
        if Path::new(&path).exists() {
            return true;
        }
    }
    false
}

/// Check if the Clevo ACPI device (HID CLV0001) is present.
fn has_clevo_acpi() -> bool {
    // ACPI devices appear at /sys/bus/acpi/devices/{HID}:XX/
    let acpi_path = format!("/sys/bus/acpi/devices/{CLEVO_ACPI_HID}:00");
    Path::new(&acpi_path).exists()
}

/// Check if the tuxedo-ec kernel module is loaded and its sysfs interface exists.
pub fn has_ec_sysfs() -> bool {
    Path::new("/sys/devices/platform/tuxedo-ec/ec_ram").exists()
}

/// Check if the upstream uniwill-laptop driver is loaded.
pub fn has_uniwill_laptop() -> bool {
    Path::new("/sys/bus/wmi/drivers/uniwill-laptop/").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_nb05_pulse1403() {
        let dev = match_nb05("PULSE1403");
        assert_eq!(dev.name, "TUXEDO Pulse 14 Gen3");
        assert_eq!(dev.platform, Platform::Nb05);
        let nb05 = dev.nb05_data.unwrap();
        assert_eq!(nb05.num_fans, 2);
        assert!(!nb05.fanctl_onereg);
    }

    #[test]
    fn test_match_nb05_infinityflex() {
        let dev = match_nb05("IFLX14I01");
        assert_eq!(dev.name, "TUXEDO InfinityFlex 14 Gen1");
        let nb05 = dev.nb05_data.unwrap();
        assert_eq!(nb05.num_fans, 1);
        assert!(nb05.fanctl_onereg);
    }

    #[test]
    fn test_match_nb05_unknown() {
        let dev = match_nb05("PULSE9999");
        assert_eq!(dev.platform, Platform::Nb05);
        assert!(dev.name.contains("unknown"));
    }

    #[test]
    fn test_is_clevo_board() {
        assert!(is_clevo_board("N850EL"));
        assert!(is_clevo_board("P650RS"));
        assert!(!is_clevo_board("GMxPxxx"));
        assert!(!is_clevo_board("PF5PU1G"));
    }
}
