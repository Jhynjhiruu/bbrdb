use std::time::Duration;

use rusb::{Device, DeviceHandle, DeviceList, GlobalContext, UsbContext};

use crate::{
    constants::{
        BB_PRODUCT_ID, IQUE_VENDOR_ID, RDB_BULK_EP_IN, RDB_BULK_EP_OUT, RDB_CONF_DESCRIPTOR,
        RDB_INTERFACE, RDB_VENDOR_ID,
    },
    error::*,
    Handle,
};

pub type GlobalHandle = Handle<GlobalContext>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RDBType {
    Retail,
    Emsmon,
    Unknown,
}

pub fn bbp_type<C: UsbContext>(device: &Device<C>) -> Result<RDBType> {
    let desc = device.device_descriptor()?;

    match (desc.vendor_id(), desc.product_id()) {
        (IQUE_VENDOR_ID, BB_PRODUCT_ID) => Ok(RDBType::Retail),
        (RDB_VENDOR_ID, BB_PRODUCT_ID) => Ok(RDBType::Emsmon),
        _ => Ok(RDBType::Unknown),
    }
}

pub fn scan_devices_in<C: UsbContext>(context: C) -> Result<Vec<Device<C>>> {
    Ok(DeviceList::new_with_context(context)?
        .iter()
        .filter(|d| bbp_type(d).is_ok_and(|t| t != RDBType::Unknown))
        .collect())
}

pub fn scan_devices() -> Result<Vec<Device<GlobalContext>>> {
    scan_devices_in(GlobalContext::default())
}

fn is_correct_descriptor<C: UsbContext>(device: &Device<C>) -> Result<bool> {
    match device.active_config_descriptor() {
        Ok(d) => Ok(d.number() == RDB_CONF_DESCRIPTOR),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn open_device<C: UsbContext>(device: &Device<C>) -> Result<DeviceHandle<C>> {
    let handle = device.open()?;

    #[cfg(not(target_os = "windows"))]
    if rusb::supports_detach_kernel_driver() && handle.kernel_driver_active(RDB_INTERFACE)? {
        handle.detach_kernel_driver(RDB_INTERFACE)?;
    }

    handle.set_active_configuration(RDB_CONF_DESCRIPTOR)?;
    if !is_correct_descriptor(device)? {
        return Err(LibBBRDBError::IncorrectDescriptor);
    }

    handle.claim_interface(RDB_INTERFACE)?;
    handle.clear_halt(RDB_BULK_EP_IN)?;
    handle.clear_halt(RDB_BULK_EP_OUT)?;

    if !is_correct_descriptor(device)? {
        return Err(LibBBRDBError::IncorrectDescriptor);
    }

    Ok(handle)
}

impl<C: UsbContext> Handle<C> {
    pub(crate) fn bulk_transfer_send(&self, data: &[u8], timeout: Duration) -> Result<usize> {
        //println!("raw send: {data:02X?}");
        wrap_libusb_error(self.handle.write_bulk(RDB_BULK_EP_OUT, data, timeout))
    }

    pub(crate) fn bulk_transfer_receive(&self, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mut buf = vec![0; len];

        match self.handle.read_bulk(RDB_BULK_EP_IN, &mut buf, timeout) {
            Ok(n) => {
                //println!("recv {:x?}", &buf[..n]);
                Ok(buf[..n].to_vec())
            }
            Err(e) => Err(e.into()),
        }
    }
}
