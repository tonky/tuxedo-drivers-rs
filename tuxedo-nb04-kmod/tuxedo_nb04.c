// SPDX-License-Identifier: GPL-2.0+
/*
 * Sensor and power profile shim for TUXEDO NB04 WMI devices.
 *
 * Uses the NB04 WMI BS interface (GUID 1F174999-3A4E-4311-900D-7BE7166D5055).
 * All WMI methods take 8-byte input and return 80-byte output buffer.
 * Output bytes [0..1] are a status u16 LE that must equal 0 (success).
 *
 * NB04 has NO direct fan PWM control — fans are governed by profile selection.
 *
 * sysfs attributes at /sys/devices/platform/tuxedo-nb04/:
 *   cpu_temp        (RO) - CPU temperature in degrees C (method 0x04, out[2])
 *   gpu_temp        (RO) - GPU temperature in degrees C (method 0x06, out[2])
 *   fan0_rpm        (RO) - Fan 1 (CPU) RPM (method 0x02, out[2..3] LE)
 *   fan1_rpm        (RO) - Fan 2 (GPU) RPM (method 0x02, out[4..5] LE)
 *   power_profile   (RW) - 0=battery, 1=balanced, 2=performance (method 0x07)
 *
 * On module unload, no cleanup needed (no manual fan mode to restore).
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/platform_device.h>
#include <linux/dmi.h>
#include <linux/wmi.h>
#include <linux/mutex.h>

#define NB04_WMI_BS_GUID	"1F174999-3A4E-4311-900D-7BE7166D5055"
#define BS_INPUT_LEN		8
#define BS_OUTPUT_LEN		80

static DEFINE_MUTEX(nb04_lock);

/* Call a WMI BS method: 8 bytes in, 80 bytes out (ACPI_TYPE_BUFFER). */
static int nb04_wmi_call(u32 method_id, u8 *in, u8 *out)
{
	struct acpi_buffer acpi_in = { BS_INPUT_LEN, in };
	struct acpi_buffer acpi_out = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_status status;
	u16 wmi_return;

	mutex_lock(&nb04_lock);
	status = wmi_evaluate_method(NB04_WMI_BS_GUID, 0, method_id,
				     &acpi_in, &acpi_out);
	mutex_unlock(&nb04_lock);

	if (ACPI_FAILURE(status))
		return -EIO;

	obj = acpi_out.pointer;
	if (!obj)
		return -ENODATA;

	if (obj->type != ACPI_TYPE_BUFFER || obj->buffer.length < BS_OUTPUT_LEN) {
		kfree(obj);
		return -EINVAL;
	}

	memcpy(out, obj->buffer.pointer, BS_OUTPUT_LEN);
	kfree(obj);

	/* Validate status word: out[0..1] LE must be 0 */
	wmi_return = out[0] | ((u16)out[1] << 8);
	if (wmi_return != 0)
		return -EIO;

	return 0;
}

/* --- sysfs: cpu_temp --- */

static ssize_t cpu_temp_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 in[BS_INPUT_LEN] = {};
	u8 out[BS_OUTPUT_LEN];
	int ret;

	ret = nb04_wmi_call(0x04, in, out);
	if (ret)
		return ret;

	return sysfs_emit(buf, "%u\n", out[2]);
}
static DEVICE_ATTR_RO(cpu_temp);

/* --- sysfs: gpu_temp --- */

static ssize_t gpu_temp_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 in[BS_INPUT_LEN] = {};
	u8 out[BS_OUTPUT_LEN];
	int ret;

	ret = nb04_wmi_call(0x06, in, out);
	if (ret)
		return ret;

	return sysfs_emit(buf, "%u\n", out[2]);
}
static DEVICE_ATTR_RO(gpu_temp);

/* --- sysfs: fan0_rpm / fan1_rpm --- */

static ssize_t fan0_rpm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 in[BS_INPUT_LEN] = {};
	u8 out[BS_OUTPUT_LEN];
	u16 rpm;
	int ret;

	ret = nb04_wmi_call(0x02, in, out);
	if (ret)
		return ret;

	rpm = out[2] | ((u16)out[3] << 8);
	return sysfs_emit(buf, "%u\n", rpm);
}
static DEVICE_ATTR_RO(fan0_rpm);

static ssize_t fan1_rpm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 in[BS_INPUT_LEN] = {};
	u8 out[BS_OUTPUT_LEN];
	u16 rpm;
	int ret;

	ret = nb04_wmi_call(0x02, in, out);
	if (ret)
		return ret;

	rpm = out[4] | ((u16)out[5] << 8);
	return sysfs_emit(buf, "%u\n", rpm);
}
static DEVICE_ATTR_RO(fan1_rpm);

/* --- sysfs: power_profile (RW) --- */

static ssize_t power_profile_show(struct device *dev,
				  struct device_attribute *attr, char *buf)
{
	/*
	 * NB04 firmware has no read-back for current profile.
	 * Return a hint that reading is not supported.
	 * The Rust daemon caches the last-written value.
	 */
	return sysfs_emit(buf, "-1\n");
}

static ssize_t power_profile_store(struct device *dev,
				   struct device_attribute *attr,
				   const char *buf, size_t count)
{
	u8 in[BS_INPUT_LEN] = {};
	u8 out[BS_OUTPUT_LEN];
	u8 mode;
	int ret;

	if (kstrtou8(buf, 0, &mode))
		return -EINVAL;
	if (mode > 2)
		return -EINVAL;

	in[0] = mode;
	ret = nb04_wmi_call(0x07, in, out);

	return ret ? ret : count;
}
static DEVICE_ATTR_RW(power_profile);

/* --- attribute group --- */

static struct attribute *nb04_attrs[] = {
	&dev_attr_cpu_temp.attr,
	&dev_attr_gpu_temp.attr,
	&dev_attr_fan0_rpm.attr,
	&dev_attr_fan1_rpm.attr,
	&dev_attr_power_profile.attr,
	NULL,
};
ATTRIBUTE_GROUPS(nb04);

/* --- platform driver --- */

static int nb04_probe(struct platform_device *pdev)
{
	if (!wmi_has_guid(NB04_WMI_BS_GUID)) {
		pr_err("NB04 WMI BS GUID not found\n");
		return -ENODEV;
	}

	pr_info("NB04 sensor/profile shim loaded\n");
	return 0;
}

static struct platform_driver nb04_driver = {
	.driver = {
		.name		= "tuxedo-nb04",
		.dev_groups	= nb04_groups,
	},
	.probe = nb04_probe,
};

static struct platform_device *nb04_device;

static const struct dmi_system_id nb04_dmi_table[] = {
	{
		.ident = "TUXEDO NB04",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
		},
	},
	{ },
};
MODULE_DEVICE_TABLE(dmi, nb04_dmi_table);

static int __init nb04_init(void)
{
	if (!dmi_check_system(nb04_dmi_table))
		return -ENODEV;

	nb04_device = platform_create_bundle(&nb04_driver,
					     nb04_probe,
					     NULL, 0, NULL, 0);
	return PTR_ERR_OR_ZERO(nb04_device);
}

static void __exit nb04_exit(void)
{
	platform_device_unregister(nb04_device);
	platform_driver_unregister(&nb04_driver);
}

module_init(nb04_init);
module_exit(nb04_exit);

MODULE_AUTHOR("TUXEDO Computers GmbH <tux@tuxedocomputers.com>");
MODULE_DESCRIPTION("TUXEDO NB04 sensor and power profile shim (WMI BS)");
MODULE_LICENSE("GPL");
