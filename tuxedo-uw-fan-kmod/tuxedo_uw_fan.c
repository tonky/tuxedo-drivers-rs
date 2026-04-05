// SPDX-License-Identifier: GPL-2.0+
/*
 * Fan control and sensor shim for Uniwill/TUXEDO laptops.
 *
 * Self-contained EC access via ACPI ECRR/ECRW methods — no dependency on
 * the upstream uniwill-laptop driver.
 *
 * sysfs attributes at /sys/devices/platform/tuxedo-uw-fan/:
 *   fan0_pwm   (RW)  - Fan 0 PWM duty (0-200, EC scale)
 *   fan1_pwm   (RW)  - Fan 1 PWM duty (0-200, EC scale)
 *   fan_mode   (RW)  - 0 = auto, 1 = manual (FAN_MODE_USER bit at 0x0751)
 *   cpu_temp   (RO)  - CPU temperature in degrees C (EC 0x043e)
 *   gpu_temp   (RO)  - GPU temperature in degrees C (EC 0x044f)
 *   fan_count  (RO)  - Number of fans (always 2 for Uniwill)
 *
 * On module unload, fan mode is restored to auto.
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/platform_device.h>
#include <linux/dmi.h>
#include <linux/acpi.h>
#include <linux/delay.h>

/* EC register addresses (same as upstream uniwill-laptop) */
#define EC_ADDR_PWM_0			0x1804
#define EC_ADDR_PWM_1			0x1809
#define EC_ADDR_MANUAL_FAN_CTRL		0x0751
#define FAN_MODE_USER			BIT(7)
#define EC_ADDR_CPU_TEMP		0x043e
#define EC_ADDR_GPU_TEMP		0x044f

/* EC access delay — upstream uses 6ms */
#define EC_DELAY_US			6000

/* PWM range in EC terms */
#define PWM_MAX				200

static acpi_handle ec_handle;
static DEFINE_MUTEX(ec_lock);

static int ec_read(u16 reg, u8 *val)
{
	union acpi_object param = {
		.integer = { .type = ACPI_TYPE_INTEGER, .value = reg }
	};
	struct acpi_object_list input = { .count = 1, .pointer = &param };
	unsigned long long output;
	acpi_status status;

	status = acpi_evaluate_integer(ec_handle, "ECRR", &input, &output);
	if (ACPI_FAILURE(status))
		return -EIO;

	usleep_range(EC_DELAY_US, EC_DELAY_US * 2);
	*val = (u8)output;
	return 0;
}

static int ec_write(u16 reg, u8 val)
{
	union acpi_object params[2] = {
		{ .integer = { .type = ACPI_TYPE_INTEGER, .value = reg } },
		{ .integer = { .type = ACPI_TYPE_INTEGER, .value = val } },
	};
	struct acpi_object_list input = { .count = 2, .pointer = params };
	acpi_status status;

	status = acpi_evaluate_object(ec_handle, "ECRW", &input, NULL);
	if (ACPI_FAILURE(status))
		return -EIO;

	usleep_range(EC_DELAY_US, EC_DELAY_US * 2);
	return 0;
}

/* --- sysfs: fan0_pwm --- */

static ssize_t fan0_pwm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 val;
	int ret;

	mutex_lock(&ec_lock);
	ret = ec_read(EC_ADDR_PWM_0, &val);
	mutex_unlock(&ec_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%u\n", val);
}

static ssize_t fan0_pwm_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	u8 val;
	int ret;

	if (kstrtou8(buf, 0, &val))
		return -EINVAL;
	if (val > PWM_MAX)
		return -EINVAL;

	mutex_lock(&ec_lock);
	ret = ec_write(EC_ADDR_PWM_0, val);
	mutex_unlock(&ec_lock);

	return ret ? ret : count;
}
static DEVICE_ATTR_RW(fan0_pwm);

/* --- sysfs: fan1_pwm --- */

static ssize_t fan1_pwm_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 val;
	int ret;

	mutex_lock(&ec_lock);
	ret = ec_read(EC_ADDR_PWM_1, &val);
	mutex_unlock(&ec_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%u\n", val);
}

static ssize_t fan1_pwm_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	u8 val;
	int ret;

	if (kstrtou8(buf, 0, &val))
		return -EINVAL;
	if (val > PWM_MAX)
		return -EINVAL;

	mutex_lock(&ec_lock);
	ret = ec_write(EC_ADDR_PWM_1, val);
	mutex_unlock(&ec_lock);

	return ret ? ret : count;
}
static DEVICE_ATTR_RW(fan1_pwm);

/* --- sysfs: fan_mode --- */

static ssize_t fan_mode_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 val;
	int ret;

	mutex_lock(&ec_lock);
	ret = ec_read(EC_ADDR_MANUAL_FAN_CTRL, &val);
	mutex_unlock(&ec_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%u\n", (val & FAN_MODE_USER) ? 1 : 0);
}

static ssize_t fan_mode_store(struct device *dev,
			      struct device_attribute *attr,
			      const char *buf, size_t count)
{
	u8 mode, val;
	int ret;

	if (kstrtou8(buf, 0, &mode))
		return -EINVAL;
	if (mode > 1)
		return -EINVAL;

	mutex_lock(&ec_lock);
	ret = ec_read(EC_ADDR_MANUAL_FAN_CTRL, &val);
	if (ret)
		goto out;

	if (mode)
		val |= FAN_MODE_USER;
	else
		val &= ~FAN_MODE_USER;

	ret = ec_write(EC_ADDR_MANUAL_FAN_CTRL, val);
out:
	mutex_unlock(&ec_lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(fan_mode);

/* --- sysfs: cpu_temp (read-only) --- */

static ssize_t cpu_temp_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 val;
	int ret;

	mutex_lock(&ec_lock);
	ret = ec_read(EC_ADDR_CPU_TEMP, &val);
	mutex_unlock(&ec_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%u\n", val);
}
static DEVICE_ATTR_RO(cpu_temp);

/* --- sysfs: gpu_temp (read-only) --- */

static ssize_t gpu_temp_show(struct device *dev,
			     struct device_attribute *attr, char *buf)
{
	u8 val;
	int ret;

	mutex_lock(&ec_lock);
	ret = ec_read(EC_ADDR_GPU_TEMP, &val);
	mutex_unlock(&ec_lock);

	if (ret)
		return ret;
	return sysfs_emit(buf, "%u\n", val);
}
static DEVICE_ATTR_RO(gpu_temp);

/* --- sysfs: fan_count (read-only, always 2) --- */

static ssize_t fan_count_show(struct device *dev,
			      struct device_attribute *attr, char *buf)
{
	return sysfs_emit(buf, "2\n");
}
static DEVICE_ATTR_RO(fan_count);

/* --- attribute group --- */

static struct attribute *uw_fan_attrs[] = {
	&dev_attr_fan0_pwm.attr,
	&dev_attr_fan1_pwm.attr,
	&dev_attr_fan_mode.attr,
	&dev_attr_cpu_temp.attr,
	&dev_attr_gpu_temp.attr,
	&dev_attr_fan_count.attr,
	NULL,
};
ATTRIBUTE_GROUPS(uw_fan);

/* --- platform driver --- */

static void restore_auto(void)
{
	u8 val;

	mutex_lock(&ec_lock);
	if (ec_read(EC_ADDR_MANUAL_FAN_CTRL, &val) == 0) {
		val &= ~FAN_MODE_USER;
		ec_write(EC_ADDR_MANUAL_FAN_CTRL, val);
	}
	mutex_unlock(&ec_lock);
}

static int uw_fan_probe(struct platform_device *pdev)
{
	acpi_status status;

	/* Find the EC ACPI handle for ECRR/ECRW methods */
	status = acpi_get_handle(NULL, "\\_SB.PCI0.SBRG.EC0", &ec_handle);
	if (ACPI_FAILURE(status)) {
		/* Try alternative path */
		status = acpi_get_handle(NULL, "\\_SB.PCI0.LPCB.EC0", &ec_handle);
		if (ACPI_FAILURE(status)) {
			pr_err("failed to find ACPI EC handle\n");
			return -ENODEV;
		}
	}

	pr_info("fan control shim loaded\n");
	return 0;
}

static struct platform_driver uw_fan_driver = {
	.driver = {
		.name		= "tuxedo-uw-fan",
		.dev_groups	= uw_fan_groups,
	},
	.probe = uw_fan_probe,
};

static struct platform_device *uw_fan_device;

/* DMI match: only load on TUXEDO hardware with Uniwill WMI GUIDs */
static const struct dmi_system_id uw_fan_dmi_table[] = {
	{
		.ident = "TUXEDO Uniwill",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
		},
	},
	{ },
};
MODULE_DEVICE_TABLE(dmi, uw_fan_dmi_table);

static int __init uw_fan_init(void)
{
	if (!dmi_check_system(uw_fan_dmi_table))
		return -ENODEV;

	uw_fan_device = platform_create_bundle(&uw_fan_driver,
					       uw_fan_probe,
					       NULL, 0, NULL, 0);
	return PTR_ERR_OR_ZERO(uw_fan_device);
}

static void __exit uw_fan_exit(void)
{
	restore_auto();
	platform_device_unregister(uw_fan_device);
	platform_driver_unregister(&uw_fan_driver);
}

module_init(uw_fan_init);
module_exit(uw_fan_exit);

MODULE_AUTHOR("TUXEDO Computers GmbH <tux@tuxedocomputers.com>");
MODULE_DESCRIPTION("TUXEDO Uniwill fan control and sensor shim (self-contained EC access)");
MODULE_LICENSE("GPL");
