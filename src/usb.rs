use std::time::Duration;

use rusb::{Device, DeviceHandle, GlobalContext};

use crate::{
    constants::{
        BB_PRODUCT_ID, IQUE_VENDOR_ID, RDB_BULK_EP_IN, RDB_BULK_EP_OUT, RDB_CONF_DESCRIPTOR,
        RDB_INTERFACE,
    },
    error::{wrap_libusb_error, LibBBRDBError, Result},
    BBPlayer,
};

impl BBPlayer {
    pub fn is_bbp(device: &Device<GlobalContext>) -> Result<bool> {
        let desc = wrap_libusb_error(device.device_descriptor())?;

        Ok(desc.vendor_id() == IQUE_VENDOR_ID && desc.product_id() == BB_PRODUCT_ID)
    }

    fn is_correct_descriptor(device: &Device<GlobalContext>) -> Result<bool> {
        match device.active_config_descriptor() {
            Ok(d) => Ok(d.number() == RDB_CONF_DESCRIPTOR),
            Err(e) => Err(e.into()),
        }
    }

    pub fn open_device(device: &Device<GlobalContext>) -> Result<DeviceHandle<GlobalContext>> {
        let mut handle = device.open()?;

        #[cfg(not(target_os = "windows"))]
        if rusb::supports_detach_kernel_driver() && handle.kernel_driver_active(RDB_INTERFACE)? {
            handle.detach_kernel_driver(RDB_INTERFACE)?;
        }

        handle.set_active_configuration(RDB_CONF_DESCRIPTOR)?;

        if !Self::is_correct_descriptor(device)? {
            return Err(LibBBRDBError::IncorrectDescriptor);
        }

        handle.claim_interface(RDB_INTERFACE)?;
        handle.clear_halt(RDB_BULK_EP_IN)?;
        handle.clear_halt(RDB_BULK_EP_OUT)?;

        if !Self::is_correct_descriptor(device)? {
            return Err(LibBBRDBError::IncorrectDescriptor);
        }

        Ok(handle)
    }

    pub fn close_connection(&mut self) -> Result<()> {
        self.handle.release_interface(RDB_INTERFACE)?;
        #[cfg(not(target_os = "windows"))]
        if rusb::supports_detach_kernel_driver() {
            self.handle.attach_kernel_driver(RDB_INTERFACE)?;
        }
        Ok(())
    }

    pub fn bulk_transfer_send<T: AsRef<[u8]>>(&self, data: T, timeout: Duration) -> Result<usize> {
        //println!("send {:x?}", data.as_ref());
        wrap_libusb_error(
            self.handle
                .write_bulk(RDB_BULK_EP_OUT, data.as_ref(), timeout),
        )
    }

    pub fn bulk_transfer_receive(&self, length: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mut buf = vec![0; length];
        //println!("expc {length:x}");
        match self.handle.read_bulk(RDB_BULK_EP_IN, &mut buf, timeout) {
            Ok(n) => {
                //println!("recv {:x?}", &buf[..n]);
                Ok(buf[..n].to_vec())
            }
            Err(e) => Err(e.into()),
        }
    }
}
