use std::time::Duration;

use nusb::{
    list_devices,
    transfer::{Buffer, Bulk, In, Out},
    Device, DeviceInfo, Endpoint, Interface, MaybeFuture,
};

use crate::{
    constants::{
        BB_PRODUCT_ID, IQUE_VENDOR_ID, RDB_BULK_EP_IN, RDB_BULK_EP_OUT, RDB_CONF_DESCRIPTOR,
        RDB_INTERFACE, RDB_VENDOR_ID,
    },
    error::*,
    Handle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RDBType {
    Retail,
    Emsmon,
    Unknown,
}

pub fn bbp_type(device: &DeviceInfo) -> Result<RDBType> {
    match (device.vendor_id(), device.product_id()) {
        (IQUE_VENDOR_ID, BB_PRODUCT_ID) => Ok(RDBType::Retail),
        (RDB_VENDOR_ID, BB_PRODUCT_ID) => Ok(RDBType::Emsmon),
        _ => Ok(RDBType::Unknown),
    }
}

pub fn scan_devices() -> Result<Vec<DeviceInfo>> {
    Ok(list_devices()
        .wait()?
        .filter(|d| bbp_type(d).is_ok_and(|t| t != RDBType::Unknown))
        .collect())
}

fn is_correct_descriptor(device: &Device) -> Result<bool> {
    match device.active_configuration() {
        Ok(d) => Ok(d.configuration_value() == RDB_CONF_DESCRIPTOR),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn open_device(
    device: &DeviceInfo,
) -> Result<(Device, Interface, Endpoint<Bulk, In>, Endpoint<Bulk, Out>)> {
    let handle = device.open().wait()?;

    #[cfg(not(target_os = "windows"))]
    {
        //handle.detach_kernel_driver(RDB_INTERFACE)?;
        handle.attach_kernel_driver(RDB_INTERFACE)?;
    }

    handle.set_configuration(RDB_CONF_DESCRIPTOR).wait()?;
    if !is_correct_descriptor(&handle)? {
        return Err(LibBBRDBError::IncorrectDescriptor);
    }

    let iface = handle.claim_interface(RDB_INTERFACE).wait()?;
    let mut ep_in = iface.endpoint(RDB_BULK_EP_IN)?;
    let mut ep_out = iface.endpoint(RDB_BULK_EP_OUT)?;
    ep_in.clear_halt().wait()?;
    ep_out.clear_halt().wait()?;

    if !is_correct_descriptor(&handle)? {
        return Err(LibBBRDBError::IncorrectDescriptor);
    }

    Ok((handle, iface, ep_in, ep_out))
}

fn wrap_nusb_transfer_error<T>(
    value: std::result::Result<T, nusb::transfer::TransferError>,
) -> Result<T> {
    value.map_err(nusb::transfer::TransferError::into)
}

impl Handle {
    pub(crate) fn bulk_transfer_send(&mut self, data: &[u8], timeout: Duration) -> Result<usize> {
        let mut buf = Buffer::new(data.len());
        buf.extend_from_slice(data);
        //println!("raw send: {data:02X?}");
        self.ep_out.submit(buf);
        let completion = self
            .ep_out
            .wait_next_complete(timeout)
            .ok_or(LibBBRDBError::Timeout(timeout))?;
        wrap_nusb_transfer_error(completion.status.map(|()| completion.actual_len))
    }

    pub(crate) fn bulk_transfer_receive(
        &mut self,
        len: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        while len > self.buf_in.len() {
            let buf = Buffer::new(self.ep_in.max_packet_size());
            self.ep_in.submit(buf);
            let completion = self
                .ep_in
                .wait_next_complete(timeout)
                .ok_or(LibBBRDBError::Timeout(timeout))?;
            completion.status?;
            //println!("got {:x?}", &completion.buffer[..completion.actual_len]);
            self.buf_in
                .extend(&completion.buffer[..completion.actual_len]);
        }

        let chunk = self.buf_in.drain(..len).collect();
        //println!("recv {:x?}", chunk);
        Ok(chunk)
    }
}
