use thiserror::Error;

use crate::commands::Command;

use crate::player_comms::TransferCommand;

#[derive(Error, Debug)]
pub enum LibBBRDBError {
    #[error("libusb error: {0}")]
    LibUSBError(#[from] rusb::Error),

    #[error("binrw error: {0}")]
    BinRWError(#[from] binrw::Error),

    #[error("Device not initialised. Did you call Init?")]
    NoConsole,

    #[error("No valid filesystem found.")]
    FS,

    #[error("Failed to read block {0} after 5 attempts")]
    ReadBlock(u32),

    #[error("Failed to write block {0} after 5 attempts")]
    WriteBlock(u32),

    #[error("Command {0:?} returned {1}")]
    Command(Command, i32),

    #[error("Write block: returned {0} (error)")]
    CheckBlockWrite(i32),

    #[error("Init FS: returned {0} (error)")]
    InitFS(i32),

    #[error("Set time: returned {0} (error)")]
    SetTime(i32),

    #[error("Get BBID: returned {0} (error)")]
    GetBBID(i32),

    #[error("Expected transfer length {0}, got {1}")]
    TransferLength(usize, usize),

    #[error("Piecemeal chunk too short (expected {} byte{}, got {} byte{})", .0 - TransferCommand::PiecemealChunkRecv as u8 + 1, if .0 != &0 {"s"} else {""}, .1 - TransferCommand::PiecemealChunkRecv as u8 + 1, if .1 != &0 {"s"} else {""})]
    PiecemealChunkTooShort(u8, u8),

    #[error("Piecemeal chunks should start with 0x1D, 0x1E or 0x1F, not 0x{0:02X}")]
    UnexpectedPiecemealChunkType(u8),

    #[error("The device has an incorrect descriptor active")]
    IncorrectDescriptor,

    #[error("Incorrect data length reply received; expected 4 bytes beginning 0x1B, received {} byte{}{}", .1, if .1 != &1 {"s"} else {""}, if let Some(b) = .0 {format!(" beginning 0x{:02X}", b)} else {"".to_string()})]
    IncorrectDataLengthReply(Option<u8>, usize),

    #[error("Incorrect reply length; expected {} byte{}, received {} byte{}", .0, if .0 != &1 {"s"} else {""}, .1, if .1 != &1 {"s"} else {""})]
    InvalidReplyLength(usize, usize),

    #[error(
        "Provided filename ({0}) is too long for the filesystem; filenames must be 8.3 DOS format"
    )]
    FileNameTooLong(String),

    #[error("Provided filename ({0}) is an invalid CString. Does it contain null bytes (0x00)?")]
    FileNameCString(String),

    #[error("No FS block found. Did the console initialise properly?")]
    NoFSBlock,

    #[error("File {0} not found on the console")]
    FileNotFound(String),

    #[error("Trying to write an invalid number of blocks; expected {} block{}, counted {}, trying to write {}", .0, if .0 != &1 {"s"} else {""}, .1, .2)]
    IncorrectNumBlocks(usize, usize, usize),

    #[error("You can only write up to 409 files to the console at once. Try deleting some first.")]
    NoEmptyFileSlots,

    #[error("There are not enough blocks free on the console. Try deleting some files to free up space.")]
    NoFreeBlocks,

    #[error("Failed to verify file {0} (expected checksum {1:08X}")]
    ChecksumFailed(String, u32),

    #[error("Invalid RDB command {0}")]
    InvalidRDBCommand(u8),
}

pub type Result<T> = std::result::Result<T, LibBBRDBError>;

pub(crate) fn wrap_libusb_error<T>(value: rusb::Result<T>) -> Result<T> {
    match value {
        Ok(v) => Ok(v),
        Err(e) => Err(e.into()),
    }
}
