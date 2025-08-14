use thiserror::Error;

use crate::rdb::RDBCommand;

pub type Result<T> = std::result::Result<T, LibBBRDBError>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CardError {
    #[error("Card not present")]
    NotPresent,

    #[error("Operation failed")]
    Failure,

    #[error("Operation invalid")]
    Invalid,

    #[error("Card changed")]
    Changed,

    #[error("FS not initialised")]
    FSNotInit,

    #[error("File exists")]
    FileExists,

    #[error("Card full")]
    CardFull,

    #[error("Not found")]
    NotFound,

    #[error("Save data invalid")]
    StateInvalid,

    #[error("Save data limit reached")]
    StateLimit,

    #[error("Unknown card error: {0}")]
    Unknown(i32),

    #[error("Bad block")]
    BadBlock(Vec<u8>, Vec<u8>),
}

impl CardError {
    pub fn from_i32(error: i32) -> Self {
        match error {
            -1 => Self::NotPresent,
            -2 => Self::Failure,
            -3 => Self::Invalid,
            -4 => Self::Changed,
            -5 => Self::FSNotInit,
            -6 => Self::FileExists,
            -7 => Self::CardFull,
            -8 => Self::NotFound,
            -9 => Self::StateInvalid,
            -10 => Self::StateLimit,
            x => Self::Unknown(x),
        }
    }
}

#[derive(Debug, Error)]
pub enum LibBBRDBError {
    #[error("libusb error: {0}")]
    LibUSBError(#[from] rusb::Error),

    #[error("binrw error: {0}")]
    BinRWError(#[from] binrw::Error),

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Device not initialised. Did you call Init?")]
    NotInitialised,

    #[error("The device has an incorrect descriptor active")]
    IncorrectDescriptor,

    #[error("Incorrect amount of data transferred")]
    WrongDataLength,

    #[error("Unknown RDB command: {0:02X}")]
    RDBUnknown(u8),

    #[error("Unhandled RDB command: {0:?}")]
    RDBUnhandled(RDBCommand),

    #[error("Incorrect command response (got {0:08X}, expected {1:08X})")]
    IncorrectCmdResponse(i32, i32),

    #[error("Console not ready for data")]
    PlayerNotReady,

    #[error("Unexpected RDB command (got {0:?}, expected one of {1:?}")]
    RDBUnexpected(RDBCommand, Vec<RDBCommand>),

    #[error("Card size must be a multiple of 4096 blocks")]
    UnhandledCardSize,

    #[error("Card error: {0}")]
    CardError(#[from] CardError),

    #[error("Invalid FAT checksum: {0:04X}")]
    InvalidFATChecksum(u16),

    #[error("No valid FATs were found")]
    NoFAT,

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Filename \"{0}\" too long (max 8.3)")]
    FileNameTooLong(String),

    #[error("Invalid filename: \"{0}\"")]
    InvalidFilename(String),

    #[error("Trying to write an invalid number of blocks; counted {0}, trying to write {1}")]
    IncorrectNumBlocks(usize, usize),

    #[error("You can only write up to 409 files to the console at once. Try deleting some first.")]
    NoEmptyFileSlots,

    #[error("There are not enough blocks free on the console. Try deleting some files to free up space.")]
    NoFreeBlocks,

    #[error("File \"{0}\" is too big to fit on the console (needed blocks: {0}, free blocks: {0}). Try deleting some files to free up space.")]
    FileTooBig(String, u32, u32),

    #[error("Failed to verify file {0} (expected checksum {1:08X})")]
    ChecksumFailed(String, u32),

    #[error("Set time: returned {0} (error)")]
    SetTime(i32),

    #[error("Bad SKSA")]
    BadSKSA,

    #[error("The provided NAND has an incorrect size (got 0x{0:X} bytes, expected 0x{1:X} bytes)")]
    InvalidNANDSize(usize, usize),

    #[error(
        "The provided spare has an incorrect size (got 0x{0:X} bytes, expected 0x{1:X} bytes)"
    )]
    InvalidSpareSize(usize, usize),
}

pub(crate) fn wrap_libusb_error<T>(value: rusb::Result<T>) -> Result<T> {
    value.map_err(rusb::Error::into)
}
