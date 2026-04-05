// SPDX-License-Identifier: GPL-2.0+
/*
 * Fan control and sensor shim for TUXEDO Tuxi ACPI devices.
 *
 * Binds to the ACPI device TUXI0000, finds TFAN subdevice, and exposes
 * fan control + sensor reads via sysfs. All policy is in the Rust daemon.
 *
 * sysfs attributes at /sys/devices/platform/tuxedo-tuxi/:
 *   fan_count  (RO)  - Number of fans (from TFAN.GCNT)
 *   fan_mode   (RW)  - 0 = auto, 1 = manual (TFAN.GMOD/SMOD)
 *   fan0_pwm   (RW)  - Fan 0 speed 0-255 (TFAN.GSPD/SSPD)
 *   fan1_pwm   (RW)  - Fan 1 speed 0-255 (TFAN.GSPD/SSPD)
 *   fan0_temp  (RO)  - Temperature in tenth-Kelvin (TFAN.GTMP)
 *   fan1_temp  (RO)  - Temperature in tenth-Kelvin (TFAN.GTMP)
 *   fan0_rpm   (RO)  - Fan 0 RPM (TFAN.GRPM)
 *   fan1_rpm   (RO)  - Fan 1 RPM (TFAN.GRPM)
 *
 * On module unload, fan mode is restored to auto.
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/platform_device.h>
#include <linux/dmi.h>
#include <linux/acpi.h>

static acpi_handle tfan_handle;
static DEFINE_MUTEX(tfan_lock);

/* Evaluate ACPI method with integer parameters, returning integer result. */
static int evaluate_int(acpi_handle handle, const char *method,
			unsigned long long *args, u32 arg_count,
			unsigned long long *result)
{
	union acpi_object *params = NULL;
	struct acpi_object_list input = { .count = arg_count };
	unsigned long long output;
	acpi_status status;
	u32 i;

	if (arg_count > 0) {
		params = kcalloc(arg_count, sizeof(*params), GFP_KERNEL);
		if (!params)
			return -ENOMEM;
		for (i = 0; i < arg_count; i++) {
			params[i].type = ACPI_TYPE_INTEGER;
			params[i].integer.value = args[i];
		}
		input.pointer = params;
	}

	status = acpi_evaluate_integer(handle, (acpi_string)method,
				       arg_count ? &input : NULL, &output);
	kfree(params);

	if (ACPI_FAILURE(status))
		return -EIO;

	if (result)
		*result = output;
	return 0;
}

/* --- sysfs: fan_count --- */

static ssize_t fan_count_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	unsigned long long count;
	int ret;

	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "GCNT", NULL, 0, &count);
	mutex_unlock(&tfan_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%llu\n", count);
}
static DEVICE_ATTR_RO(fan_count);

/* --- sysfs: fan_mode --- */

static ssize_t fan_mode_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	unsigned long long mode;
	int ret;

	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "GMOD", NULL, 0, &mode);
	mutex_unlock(&tfan_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%llu\n", mode);
}

static ssize_t fan_mode_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	unsigned long long args[1];
	u8 mode;
	int ret;

	if (kstrtou8(buf, 0, &mode))
		return -EINVAL;
	if (mode > 1)
		return -EINVAL;

	args[0] = mode;
	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "SMOD", args, 1, NULL);
	mutex_unlock(&tfan_lock);

	return ret ? ret : count;
}
static DEVICE_ATTR_RW(fan_mode);

/* --- sysfs: fan0_pwm / fan1_pwm --- */

static ssize_t show_fan_pwm(u8 index, char *buf)
{
	unsigned long long args[1] = { index };
	unsigned long long speed;
	int ret;

	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "GSPD", args, 1, &speed);
	mutex_unlock(&tfan_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%llu\n", speed);
}

static ssize_t store_fan_pwm(u8 index, const char *buf, size_t count)
{
	unsigned long long args[2];
	u8 speed;
	int ret;

	if (kstrtou8(buf, 0, &speed))
		return -EINVAL;

	args[0] = index;
	args[1] = speed;
	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "SSPD", args, 2, NULL);
	mutex_unlock(&tfan_lock);

	return ret ? ret : count;
}

static ssize_t fan0_pwm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	return show_fan_pwm(0, buf);
}

static ssize_t fan0_pwm_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	return store_fan_pwm(0, buf, count);
}
static DEVICE_ATTR_RW(fan0_pwm);

static ssize_t fan1_pwm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	return show_fan_pwm(1, buf);
}

static ssize_t fan1_pwm_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	return store_fan_pwm(1, buf, count);
}
static DEVICE_ATTR_RW(fan1_pwm);

/* --- sysfs: fan0_temp / fan1_temp (tenth-Kelvin, raw) --- */

static ssize_t show_fan_temp(u8 index, char *buf)
{
	unsigned long long args[1] = { index };
	unsigned long long temp;
	int ret;

	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "GTMP", args, 1, &temp);
	mutex_unlock(&tfan_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%llu\n", temp);
}

static ssize_t fan0_temp_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	return show_fan_temp(0, buf);
}
static DEVICE_ATTR_RO(fan0_temp);

static ssize_t fan1_temp_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	return show_fan_temp(1, buf);
}
static DEVICE_ATTR_RO(fan1_temp);

/* --- sysfs: fan0_rpm / fan1_rpm --- */

static ssize_t show_fan_rpm(u8 index, char *buf)
{
	unsigned long long args[1] = { index };
	unsigned long long rpm;
	int ret;

	mutex_lock(&tfan_lock);
	ret = evaluate_int(tfan_handle, "GRPM", args, 1, &rpm);
	mutex_unlock(&tfan_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%llu\n", rpm);
}

static ssize_t fan0_rpm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	return show_fan_rpm(0, buf);
}
static DEVICE_ATTR_RO(fan0_rpm);

static ssize_t fan1_rpm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	return show_fan_rpm(1, buf);
}
static DEVICE_ATTR_RO(fan1_rpm);

/* --- attribute group --- */

static struct attribute *tuxi_attrs[] = {
	&dev_attr_fan_count.attr,
	&dev_attr_fan_mode.attr,
	&dev_attr_fan0_pwm.attr,
	&dev_attr_fan1_pwm.attr,
	&dev_attr_fan0_temp.attr,
	&dev_attr_fan1_temp.attr,
	&dev_attr_fan0_rpm.attr,
	&dev_attr_fan1_rpm.attr,
	NULL,
};
ATTRIBUTE_GROUPS(tuxi);

/* --- platform driver --- */

static void restore_auto(void)
{
	unsigned long long args[1] = { 0 }; /* AUTO = 0 */

	mutex_lock(&tfan_lock);
	evaluate_int(tfan_handle, "SMOD", args, 1, NULL);
	mutex_unlock(&tfan_lock);
}

static int tuxi_probe(struct platform_device *pdev)
{
	acpi_status status;
	acpi_handle tuxi_dev;

	/* Find the TUXI0000 ACPI device */
	status = acpi_get_handle(NULL, "\\_SB.TUXI", &tuxi_dev);
	if (ACPI_FAILURE(status)) {
		/* Try under different paths — some BIOS put it elsewhere */
		status = acpi_get_devices("TUXI0000", NULL, NULL, &tuxi_dev);
		if (ACPI_FAILURE(status)) {
			pr_err("failed to find TUXI0000 ACPI device\n");
			return -ENODEV;
		}
	}

	/* Find TFAN subdevice */
	status = acpi_get_handle(tuxi_dev, "TFAN", &tfan_handle);
	if (ACPI_FAILURE(status)) {
		pr_err("failed to find TFAN subdevice\n");
		return -ENODEV;
	}

	pr_info("Tuxi fan control shim loaded\n");
	return 0;
}

static struct platform_driver tuxi_driver = {
	.driver = {
		.name		= "tuxedo-tuxi",
		.dev_groups	= tuxi_groups,
	},
	.probe = tuxi_probe,
};

static struct platform_device *tuxi_device;

static const struct dmi_system_id tuxi_dmi_table[] = {
	{
		.ident = "TUXEDO Tuxi",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
		},
	},
	{ },
};
MODULE_DEVICE_TABLE(dmi, tuxi_dmi_table);

static int __init tuxi_init(void)
{
	if (!dmi_check_system(tuxi_dmi_table))
		return -ENODEV;

	tuxi_device = platform_create_bundle(&tuxi_driver,
					     tuxi_probe,
					     NULL, 0, NULL, 0);
	return PTR_ERR_OR_ZERO(tuxi_device);
}

static void __exit tuxi_exit(void)
{
	restore_auto();
	platform_device_unregister(tuxi_device);
	platform_driver_unregister(&tuxi_driver);
}

module_init(tuxi_init);
module_exit(tuxi_exit);

MODULE_AUTHOR("TUXEDO Computers GmbH <tux@tuxedocomputers.com>");
MODULE_DESCRIPTION("TUXEDO Tuxi fan control and sensor shim (ACPI TFAN)");
MODULE_LICENSE("GPL");
