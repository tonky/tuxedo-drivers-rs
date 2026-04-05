// SPDX-License-Identifier: GPL-2.0+
/*
 * Fan control shim for Clevo/TUXEDO ACPI/WMI devices.
 *
 * Dual-transport: tries ACPI DSM (HID CLV0001) first, falls back to WMI
 * (GUID ABBC0F6D-8EA1-11D1-00A0-C90629100000). Validated on probe by
 * calling cmd 0x52 (GET_BIOS_FEATURES_1) and checking for non-0xffffffff.
 *
 * sysfs attributes at /sys/devices/platform/tuxedo-clevo/:
 *   fan0_info  (RO)  - FANINFO1 (cmd 0x63), raw u32
 *   fan1_info  (RO)  - FANINFO2 (cmd 0x64), raw u32
 *   fan2_info  (RO)  - FANINFO3 (cmd 0x6e), raw u32
 *   fan_speed  (WO)  - Set fan speeds (cmd 0x68), packed u32
 *   fan_auto   (WO)  - Restore auto fan control (cmd 0x69)
 *
 * FANINFO u32 layout (parsed in userspace):
 *   bits [7:0]   = fan duty (0-255)
 *   bits [15:8]  = temperature (degrees C)
 *   bits [31:16] = RPM
 *
 * fan_speed packed u32 layout:
 *   bits [7:0]   = fan0 duty
 *   bits [15:8]  = fan1 duty
 *   bits [23:16] = fan2 duty
 *
 * On module unload, fan mode is restored to auto.
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/platform_device.h>
#include <linux/dmi.h>
#include <linux/acpi.h>
#include <linux/wmi.h>
#include <linux/delay.h>
#include <linux/mutex.h>

#define CLEVO_WMI_METHOD_GUID	"ABBC0F6D-8EA1-11D1-00A0-C90629100000"
#define CLEVO_WMI_EVENT_GUID	"ABBC0F6B-8EA1-11D1-00A0-C90629100000"
#define CLEVO_ACPI_DSM_UUID	"93f224e4-fbdc-4bbf-add6-db71bdc0afad"
#define CLEVO_ACPI_HID		"CLV0001"

#define CMD_GET_BIOS_FEATURES_1	0x52
#define CMD_GET_FANINFO1	0x63
#define CMD_GET_FANINFO2	0x64
#define CMD_GET_FANINFO3	0x6e
#define CMD_SET_FANSPEED	0x68
#define CMD_SET_FANAUTO		0x69

enum clevo_transport {
	TRANSPORT_NONE = 0,
	TRANSPORT_WMI,
	TRANSPORT_ACPI_DSM,
};

static DEFINE_MUTEX(clevo_lock);
static enum clevo_transport transport;
static acpi_handle dsm_handle;
static guid_t dsm_uuid;

/* --- transport dispatch --- */

static int clevo_cmd_wmi(u8 cmd, u32 arg, u32 *result)
{
	struct acpi_buffer in = { sizeof(arg), &arg };
	struct acpi_buffer out = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_status status;

	status = wmi_evaluate_method(CLEVO_WMI_METHOD_GUID, 0, cmd,
				     &in, &out);
	if (ACPI_FAILURE(status))
		return -EIO;

	obj = out.pointer;
	if (!obj) {
		return -EIO;
	}

	if (obj->type == ACPI_TYPE_INTEGER) {
		if (result)
			*result = (u32)obj->integer.value;
	}

	kfree(obj);
	return 0;
}

static int clevo_cmd_dsm(u8 cmd, u32 arg, u32 *result)
{
	union acpi_object argv4_data = {
		.integer.type = ACPI_TYPE_INTEGER,
		.integer.value = arg,
	};
	union acpi_object argv4 = {
		.package.type = ACPI_TYPE_PACKAGE,
		.package.count = 1,
		.package.elements = &argv4_data,
	};
	union acpi_object *obj;

	obj = acpi_evaluate_dsm(dsm_handle, &dsm_uuid, 0, cmd, &argv4);
	if (!obj)
		return -EIO;

	if (obj->type == ACPI_TYPE_INTEGER) {
		if (result)
			*result = (u32)obj->integer.value;
	}

	ACPI_FREE(obj);
	return 0;
}

static int clevo_cmd(u8 cmd, u32 arg, u32 *result)
{
	switch (transport) {
	case TRANSPORT_WMI:
		return clevo_cmd_wmi(cmd, arg, result);
	case TRANSPORT_ACPI_DSM:
		return clevo_cmd_dsm(cmd, arg, result);
	default:
		return -ENODEV;
	}
}

/* --- sysfs: fan0_info / fan1_info / fan2_info --- */

static ssize_t show_fan_info(u8 cmd, char *buf)
{
	u32 info;
	int ret;

	mutex_lock(&clevo_lock);
	ret = clevo_cmd(cmd, 0, &info);
	mutex_unlock(&clevo_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%u\n", info);
}

static ssize_t fan0_info_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	return show_fan_info(CMD_GET_FANINFO1, buf);
}
static DEVICE_ATTR_RO(fan0_info);

static ssize_t fan1_info_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	return show_fan_info(CMD_GET_FANINFO2, buf);
}
static DEVICE_ATTR_RO(fan1_info);

static ssize_t fan2_info_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	return show_fan_info(CMD_GET_FANINFO3, buf);
}
static DEVICE_ATTR_RO(fan2_info);

/* --- sysfs: fan_speed (write-only) --- */

static ssize_t fan_speed_store(struct device *dev,
			       struct device_attribute *attr,
			       const char *buf, size_t count)
{
	u32 packed;
	int ret;

	if (kstrtou32(buf, 0, &packed))
		return -EINVAL;

	mutex_lock(&clevo_lock);
	ret = clevo_cmd(CMD_SET_FANSPEED, packed, NULL);
	if (!ret) {
		/*
		 * Hardware needs time to apply the new speed.
		 * No known ready flag; 50ms is too short, 100ms works.
		 * See vendor tuxedo_io.c.
		 */
		msleep(100);
	}
	mutex_unlock(&clevo_lock);

	return ret ? ret : count;
}
static DEVICE_ATTR_WO(fan_speed);

/* --- sysfs: fan_auto (write-only) --- */

static ssize_t fan_auto_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	int ret;

	mutex_lock(&clevo_lock);
	ret = clevo_cmd(CMD_SET_FANAUTO, 0, NULL);
	mutex_unlock(&clevo_lock);

	return ret ? ret : count;
}
static DEVICE_ATTR_WO(fan_auto);

/* --- attribute group --- */

static struct attribute *clevo_attrs[] = {
	&dev_attr_fan0_info.attr,
	&dev_attr_fan1_info.attr,
	&dev_attr_fan2_info.attr,
	&dev_attr_fan_speed.attr,
	&dev_attr_fan_auto.attr,
	NULL,
};
ATTRIBUTE_GROUPS(clevo);

/* --- transport probing --- */

static int probe_wmi(void)
{
	u32 result;
	int ret;

	if (!wmi_has_guid(CLEVO_WMI_METHOD_GUID))
		return -ENODEV;
	if (!wmi_has_guid(CLEVO_WMI_EVENT_GUID))
		return -ENODEV;

	ret = clevo_cmd_wmi(CMD_GET_BIOS_FEATURES_1, 0, &result);
	if (ret)
		return ret;
	if (result == 0xffffffff)
		return -ENODEV;

	return 0;
}

static int probe_acpi_dsm(void)
{
	acpi_status status;
	acpi_handle clv_handle;
	u32 result;
	int ret;

	/* Find CLV0001 ACPI device */
	status = acpi_get_devices(CLEVO_ACPI_HID, NULL, NULL, &clv_handle);
	if (ACPI_FAILURE(status))
		return -ENODEV;

	ret = guid_parse(CLEVO_ACPI_DSM_UUID, &dsm_uuid);
	if (ret)
		return ret;

	dsm_handle = clv_handle;
	transport = TRANSPORT_ACPI_DSM;

	ret = clevo_cmd(CMD_GET_BIOS_FEATURES_1, 0, &result);
	if (ret) {
		transport = TRANSPORT_NONE;
		return ret;
	}
	if (result == 0xffffffff) {
		transport = TRANSPORT_NONE;
		return -ENODEV;
	}

	return 0;
}

/* --- platform driver --- */

static int clevo_probe(struct platform_device *pdev)
{
	int ret;

	/* Try WMI first (more common), then ACPI DSM */
	ret = probe_wmi();
	if (!ret) {
		transport = TRANSPORT_WMI;
		pr_info("Clevo fan control shim loaded (WMI transport)\n");
		return 0;
	}

	ret = probe_acpi_dsm();
	if (!ret) {
		pr_info("Clevo fan control shim loaded (ACPI DSM transport)\n");
		return 0;
	}

	pr_err("no Clevo WMI or ACPI DSM transport found\n");
	return -ENODEV;
}

static void restore_auto(void)
{
	mutex_lock(&clevo_lock);
	clevo_cmd(CMD_SET_FANAUTO, 0, NULL);
	mutex_unlock(&clevo_lock);
}

static struct platform_driver clevo_driver = {
	.driver = {
		.name		= "tuxedo-clevo",
		.dev_groups	= clevo_groups,
	},
	.probe = clevo_probe,
};

static struct platform_device *clevo_device;

static const struct dmi_system_id clevo_dmi_table[] = {
	{
		.ident = "TUXEDO Clevo",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
		},
	},
	{ },
};
MODULE_DEVICE_TABLE(dmi, clevo_dmi_table);

static int __init clevo_init(void)
{
	if (!dmi_check_system(clevo_dmi_table))
		return -ENODEV;

	clevo_device = platform_create_bundle(&clevo_driver,
					      clevo_probe,
					      NULL, 0, NULL, 0);
	return PTR_ERR_OR_ZERO(clevo_device);
}

static void __exit clevo_exit(void)
{
	restore_auto();
	platform_device_unregister(clevo_device);
	platform_driver_unregister(&clevo_driver);
}

module_init(clevo_init);
module_exit(clevo_exit);

MODULE_AUTHOR("TUXEDO Computers GmbH <tux@tuxedocomputers.com>");
MODULE_DESCRIPTION("TUXEDO Clevo fan control shim (WMI/ACPI DSM)");
MODULE_LICENSE("GPL");
