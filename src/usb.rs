use std::time::Duration;

use rusb::{Device, DeviceHandle, Error, GlobalContext, Result};

use crate::{
    constants::{
        BB_PRODUCT_ID, IQUE_VENDOR_ID, RDB_BULK_EP_IN, RDB_BULK_EP_OUT, RDB_CONF_DESCRIPTOR,
        RDB_INTERFACE,
    },
    BBPlayer,
};

impl BBPlayer {
    pub fn is_bbp(device: &Device<GlobalContext>) -> bool {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{e}");
                return false;
            }
        };

        desc.vendor_id() == IQUE_VENDOR_ID && desc.product_id() == BB_PRODUCT_ID
    }

    fn is_correct_descriptor(device: &Device<GlobalContext>) -> bool {
        match device.active_config_descriptor() {
            Ok(d) => d.number() == RDB_CONF_DESCRIPTOR,
            Err(e) => {
                eprintln!("{e}");
                false
            }
        }
    }

    pub fn open_device(device: &Device<GlobalContext>) -> Result<DeviceHandle<GlobalContext>> {
        let mut handle = device.open()?;

        #[cfg(target_os = "linux")]
        if handle.kernel_driver_active(Self::RDB_INTERFACE)? {
            handle.detach_kernel_driver(Self::RDB_INTERFACE)?;
        }

        if !Self::is_correct_descriptor(device) {
            return Err(Error::BadDescriptor);
        }

        handle.claim_interface(RDB_INTERFACE)?;
        handle.clear_halt(RDB_BULK_EP_IN)?;
        handle.clear_halt(RDB_BULK_EP_OUT)?;

        if !Self::is_correct_descriptor(device) {
            return Err(Error::BadDescriptor);
        }

        Ok(handle)
    }

    pub fn close_connection(&mut self) -> Result<()> {
        self.handle.release_interface(RDB_INTERFACE)?;
        #[cfg(target_os = "linux")]
        self.handle.attach_kernel_driver(Self::RDB_INTERFACE)?;
        Ok(())
    }

    pub fn bulk_transfer_send<T: AsRef<[u8]>>(&self, data: T, timeout: Duration) -> Result<usize> {
        self.handle
            .write_bulk(RDB_BULK_EP_OUT, data.as_ref(), timeout)
    }

    pub fn bulk_transfer_receive(&self, length: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mut buf = vec![0; length];
        match self.handle.read_bulk(RDB_BULK_EP_IN, &mut buf, timeout) {
            Ok(n) => Ok(buf[..n].to_vec()),
            Err(e) => Err(e),
        }
    }
}
