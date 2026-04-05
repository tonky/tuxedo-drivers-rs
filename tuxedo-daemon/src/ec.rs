//! EC RAM access via the tuxedo-ec kernel module's sysfs binary attribute.
//!
//! The kernel module exposes `/sys/devices/platform/tuxedo-ec/ec_ram` as a
//! 64 KiB binary file.  Each byte offset corresponds to an EC RAM address.
//! We use `pread(2)` / `pwrite(2)` for atomic single-syscall access.

use std::fs::{File, OpenOptions};
use std::io;

use nix::sys::uio::{pread, pwrite};

const EC_RAM_PATH: &str = "/sys/devices/platform/tuxedo-ec/ec_ram";

/// Handle to the EC RAM sysfs file.
pub struct EcRam {
    file: File,
}

impl EcRam {
    /// Open the EC RAM sysfs file for read/write.
    pub fn open() -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(EC_RAM_PATH)?;
        Ok(Self { file })
    }

    /// Read a single byte from EC RAM at `addr`.
    pub fn read_byte(&self, addr: u16) -> io::Result<u8> {
        let mut buf = [0u8; 1];
        let n = pread(&self.file, &mut buf, addr as i64)
            .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "pread returned 0"));
        }
        Ok(buf[0])
    }

    /// Write a single byte to EC RAM at `addr`.
    pub fn write_byte(&self, addr: u16, val: u8) -> io::Result<()> {
        let buf = [val];
        pwrite(&self.file, &buf, addr as i64)
            .map_err(|e| io::Error::from_raw_os_error(e as i32))?;
        Ok(())
    }

    /// Read EC firmware version (major at 0x0400, minor at 0x0401).
    pub fn read_fw_version(&self) -> io::Result<(u8, u8)> {
        let major = self.read_byte(0x0400)?;
        let minor = self.read_byte(0x0401)?;
        Ok((major, minor))
    }
}
