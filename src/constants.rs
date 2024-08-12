use std::mem::size_of;
use std::time::Duration;

pub(crate) const RDB_VENDOR_ID: u16 = 0x1527;
pub(crate) const IQUE_VENDOR_ID: u16 = 0xBB3D;
pub(crate) const BB_PRODUCT_ID: u16 = 0xBBDB;

pub(crate) const RDB_CONF_DESCRIPTOR: u8 = 1;
pub(crate) const RDB_INTERFACE: u8 = 0;

pub(crate) const RDB_BULK_EP_OUT: u8 = 0x02;
pub(crate) const RDB_BULK_EP_IN: u8 = 0x82;

pub(crate) const RDB_BLOCK_SIZE: usize = 256 - 2 * size_of::<u8>();
pub(crate) const RDB_BLOCKS_PER_CHUNK: usize = 80;

pub(crate) const BLOCK_SIZE: usize = 0x4000;
pub(crate) const BLOCK_CHUNK_SIZE: usize = 0x1000;
pub(crate) const SPARE_SIZE: usize = 0x10;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) const NUM_FATS: u32 = 16;

pub(crate) const STATUS_OFFSET: usize = 5;
