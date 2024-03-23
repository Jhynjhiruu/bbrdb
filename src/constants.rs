use std::time::Duration;

//pub(crate) const IQUE_VENDOR_ID: u16 = 0x1527;
//pub(crate) const BB_PRODUCT_ID: u16 = 0xBBDB;

pub(crate) const IQUE_VENDOR_ID: u16 = 0xBB3D;
pub(crate) const BB_PRODUCT_ID: u16 = 0xBBDB;

pub(crate) const RDB_CONF_DESCRIPTOR: u8 = 1;
pub(crate) const RDB_INTERFACE: u8 = 0;

pub(crate) const RDB_BULK_EP_OUT: u8 = 0x02;
pub(crate) const RDB_BULK_EP_IN: u8 = 0x82;

pub(crate) const BLOCK_SIZE: usize = 0x4000;
pub(crate) const BLOCK_CHUNK_SIZE: usize = 0x1000;
pub(crate) const SPARE_SIZE: usize = 0x10;

pub(crate) const TIMEOUT: Duration = Duration::SECOND;

pub(crate) const PACKET_SIZE: usize = 0x80;

pub(crate) const SEND_CHUNK_SIZE: usize = 0x100;
