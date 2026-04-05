// SPDX-License-Identifier: GPL-2.0+
/*
 * Minimal kernel shim for NB05 EC access via I2EC indirect port I/O.
 *
 * Exposes a single sysfs binary attribute "ec_ram" that maps the 64 KiB
 * EC address space.  Userspace accesses individual bytes with pread/pwrite
 * at offsets 0x0000–0xFFFF.  Each syscall performs the full I2EC transaction
 * (12 outb + 1 inb for read, 12 outb for write) under a mutex.
 *
 * Copyright (c) 2024 TUXEDO Computers GmbH <tux@tuxedocomputers.com>
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/platform_device.h>
#include <linux/dmi.h>
#include <linux/sysfs.h>
#include <linux/mutex.h>
#include <asm/io.h>

/* SuperIO-style indirect I/O ports */
#define EC_PORT_ADDR	0x4e
#define EC_PORT_DATA	0x4f

/* I2EC register indices */
#define I2EC_REG_ADDR	0x2e
#define I2EC_REG_DATA	0x2f

/* I2EC address/data sub-registers */
#define I2EC_ADDR_LOW	0x10
#define I2EC_ADDR_HIGH	0x11
#define I2EC_ADDR_DATA	0x12

#define EC_RAM_SIZE	0x10000  /* 64 KiB address space */

static DEFINE_MUTEX(ec_lock);

static void i2ec_write(u8 reg, u8 val)
{
	outb(reg, EC_PORT_ADDR);
	outb(val, EC_PORT_DATA);
}

static u8 i2ec_read(u8 reg)
{
	outb(reg, EC_PORT_ADDR);
	return inb(EC_PORT_DATA);
}

static void ec_set_addr(u16 addr)
{
	i2ec_write(I2EC_REG_ADDR, I2EC_ADDR_HIGH);
	i2ec_write(I2EC_REG_DATA, (addr >> 8) & 0xff);
	i2ec_write(I2EC_REG_ADDR, I2EC_ADDR_LOW);
	i2ec_write(I2EC_REG_DATA, addr & 0xff);
}

static u8 ec_read_byte(u16 addr)
{
	ec_set_addr(addr);
	i2ec_write(I2EC_REG_ADDR, I2EC_ADDR_DATA);
	return i2ec_read(I2EC_REG_DATA);
}

static void ec_write_byte(u16 addr, u8 val)
{
	ec_set_addr(addr);
	i2ec_write(I2EC_REG_ADDR, I2EC_ADDR_DATA);
	i2ec_write(I2EC_REG_DATA, val);
}

/*
 * sysfs binary attribute: /sys/devices/platform/tuxedo-ec/ec_ram
 * Read/write single bytes at offset = EC address.
 */
static ssize_t ec_ram_read(struct file *filp, struct kobject *kobj,
			   struct bin_attribute *attr,
			   char *buf, loff_t off, size_t count)
{
	size_t i;

	if (off >= EC_RAM_SIZE)
		return 0;
	if (off + count > EC_RAM_SIZE)
		count = EC_RAM_SIZE - off;

	mutex_lock(&ec_lock);
	for (i = 0; i < count; i++)
		buf[i] = ec_read_byte((u16)(off + i));
	mutex_unlock(&ec_lock);

	return count;
}

static ssize_t ec_ram_write(struct file *filp, struct kobject *kobj,
			    struct bin_attribute *attr,
			    char *buf, loff_t off, size_t count)
{
	size_t i;

	if (off >= EC_RAM_SIZE)
		return -EFBIG;
	if (off + count > EC_RAM_SIZE)
		count = EC_RAM_SIZE - off;

	mutex_lock(&ec_lock);
	for (i = 0; i < count; i++)
		ec_write_byte((u16)(off + i), buf[i]);
	mutex_unlock(&ec_lock);

	return count;
}

static BIN_ATTR_RW(ec_ram, EC_RAM_SIZE);

static struct bin_attribute *tuxedo_ec_bin_attrs[] = {
	&bin_attr_ec_ram,
	NULL,
};

static const struct attribute_group tuxedo_ec_group = {
	.bin_attrs = tuxedo_ec_bin_attrs,
};

static const struct attribute_group *tuxedo_ec_groups[] = {
	&tuxedo_ec_group,
	NULL,
};

/* DMI match table — same models as the original C driver */
static const struct dmi_system_id tuxedo_ec_dmi_table[] = {
	{
		.ident = "TUXEDO Pulse 14 Gen3",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
			DMI_MATCH(DMI_BOARD_VENDOR, "NB05"),
			DMI_MATCH(DMI_PRODUCT_SKU, "PULSE1403"),
		},
	},
	{
		.ident = "TUXEDO Pulse 14 Gen4",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
			DMI_MATCH(DMI_BOARD_VENDOR, "NB05"),
			DMI_MATCH(DMI_PRODUCT_SKU, "PULSE1404"),
		},
	},
	{
		.ident = "TUXEDO InfinityFlex 14 Gen1",
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "TUXEDO"),
			DMI_MATCH(DMI_BOARD_VENDOR, "NB05"),
			DMI_MATCH(DMI_PRODUCT_SKU, "IFLX14I01"),
		},
	},
	{ },
};
MODULE_DEVICE_TABLE(dmi, tuxedo_ec_dmi_table);

static int tuxedo_ec_probe(struct platform_device *pdev)
{
	u8 major, minor;

	major = ec_read_byte(0x0400);
	minor = ec_read_byte(0x0401);
	pr_info("EC firmware version %d.%d\n", major, minor);

	return 0;
}

static struct platform_driver tuxedo_ec_driver = {
	.driver = {
		.name		= "tuxedo-ec",
		.dev_groups	= tuxedo_ec_groups,
	},
	.probe = tuxedo_ec_probe,
};

static struct platform_device *tuxedo_ec_device;

static int __init tuxedo_ec_init(void)
{
	if (!dmi_check_system(tuxedo_ec_dmi_table))
		return -ENODEV;

	tuxedo_ec_device = platform_create_bundle(&tuxedo_ec_driver,
						  tuxedo_ec_probe,
						  NULL, 0, NULL, 0);
	return PTR_ERR_OR_ZERO(tuxedo_ec_device);
}

static void __exit tuxedo_ec_exit(void)
{
	platform_device_unregister(tuxedo_ec_device);
	platform_driver_unregister(&tuxedo_ec_driver);
}

module_init(tuxedo_ec_init);
module_exit(tuxedo_ec_exit);

MODULE_AUTHOR("TUXEDO Computers GmbH <tux@tuxedocomputers.com>");
MODULE_DESCRIPTION("TUXEDO NB05 EC I/O shim (sysfs binary attribute)");
MODULE_LICENSE("GPL");
