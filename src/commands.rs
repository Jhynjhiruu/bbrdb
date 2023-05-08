use crate::{num_from_arr, BBPlayer};
use rusb::{Error, Result};

use indicatif::ProgressIterator;

#[repr(u32)]
pub(crate) enum Command {
    WriteBlock = 0x06,
    ReadBlock = 0x07,

    WriteBlockAndSpare = 0x10,
    ReadBlockAndSpare = 0x11,
    InitFS = 0x12,

    GetNumBlocks = 0x15,
    SetSeqNo = 0x16,
    GetSeqNo = 0x17,

    FileChksum = 0x1C,
    SetLED = 0x1D,
    SetTime = 0x1E,
    GetBBID = 0x1F,
    SignHash = 0x20,
}

pub type BlockSpare = (Vec<u8>, Vec<u8>);

macro_rules! try_continue {
    ($e:expr) => {
        match $e {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        }
    };
}

impl BBPlayer {
    const BLOCK_SIZE: usize = 0x4000;
    const BLOCK_CHUNK_SIZE: usize = 0x1000;
    const SPARE_SIZE: usize = 0x10;

    fn command_error(buf: &[u8]) -> bool {
        num_from_arr::<i32, _>(&buf[4..8]) < 0
    }

    pub(super) fn read_block_spare(&self, block_num: u32) -> Result<BlockSpare> {
        // attempts
        for _ in 0..5 {
            self.request_block_read(Command::ReadBlockAndSpare, block_num)?;
            let block = try_continue!(self.get_block());
            let spare = try_continue!(self.get_spare());
            return Ok((block, spare));
        }
        Err(Error::Io)
    }

    fn request_block_read(&self, command: Command, block_num: u32) -> Result<()> {
        self.send_command(command, block_num)?;
        if Self::command_error(&self.receive_reply(8)?) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }

    fn get_block(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(Self::BLOCK_SIZE);
        for _ in 0..(Self::BLOCK_SIZE / Self::BLOCK_CHUNK_SIZE) {
            buf.extend(self.receive_reply(Self::BLOCK_CHUNK_SIZE)?);
        }
        Ok(buf)
    }

    fn get_spare(&self) -> Result<Vec<u8>> {
        self.receive_reply(Self::SPARE_SIZE)
    }

    pub(super) fn write_block_spare(
        &self,
        block: &[u8],
        spare: &[u8],
        block_num: u32,
    ) -> Result<()> {
        if spare[5] != 0xFF {
            // block is marked bad
            return Ok(());
        }

        // attempts
        for _ in 0..5 {
            try_continue!(self.request_block_write(Command::WriteBlockAndSpare, block_num));
            try_continue!(self.send_block(block));
            try_continue!(self.send_spare(spare));
            try_continue!(self.check_block_write());
            return Ok(());
        }
        Err(Error::Io)
    }

    fn request_block_write(&self, command: Command, block_num: u32) -> Result<()> {
        self.send_command(command, block_num)?;
        self.wait_ready()
    }

    fn check_block_write(&self) -> Result<()> {
        if Self::command_error(&self.receive_reply(8)?) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }

    fn send_block(&self, data: &[u8]) -> Result<()> {
        self.send_chunked_data(data)
    }

    fn send_spare(&self, data: &[u8]) -> Result<()> {
        self.wait_ready()?;
        let data = [&data[..3], &[0xFF; Self::SPARE_SIZE - 3]].concat();
        match self.send_piecemeal_data(data) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub(super) fn init_fs(&self) -> Result<()> {
        self.send_command(Command::InitFS, 0x00)?;
        if Self::command_error(&self.receive_reply(8)?) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }

    pub(super) fn get_num_blocks(&self) -> Result<u32> {
        self.send_command(Command::GetNumBlocks, 0x00)?;
        let reply = self.receive_reply(8)?;
        let size: u32 = num_from_arr(&reply[4..8]);
        Ok(size)
    }

    pub(super) fn set_seqno(&self, arg: u32) -> Result<()> {
        self.send_command(Command::SetSeqNo, arg)?;
        self.receive_reply(8)?;
        Ok(())
    }

    pub(super) fn set_led(&self, ledval: u32) -> Result<()> {
        self.send_command(Command::SetLED, ledval)?;
        self.receive_reply(8)?;
        Ok(())
    }

    pub(super) fn set_time(&self, timedata: [u8; 8]) -> Result<()> {
        let first_half = num_from_arr(*timedata.split_array_ref::<4>().0);
        let second_half = &timedata[4..];
        self.send_command(Command::SetTime, first_half)?;
        if Self::command_error(&self.receive_reply(8)?) {
            Err(Error::Io)
        } else {
            self.send_piecemeal_data(second_half)?;
            Ok(())
        }
    }

    pub(super) fn get_bbid(&self) -> Result<u32> {
        self.send_command(Command::GetBBID, 0x00)?;
        let reply = self.receive_reply(8)?;
        if Self::command_error(&reply) {
            Err(Error::Io)
        } else {
            Ok(num_from_arr(&reply[4..8]))
        }
    }

    pub(super) fn dump_nand_and_spare(&self) -> Result<BlockSpare> {
        let num_blocks = self.get_num_blocks()?;
        let mut nand = Vec::with_capacity(num_blocks as usize * Self::BLOCK_SIZE);
        let mut spare = Vec::with_capacity(num_blocks as usize * Self::SPARE_SIZE);
        for block_num in (0..num_blocks).progress() {
            let (dumped_block, dumped_spare) = self.read_block_spare(block_num)?;
            nand.extend(dumped_block);
            spare.extend(dumped_spare);
        }
        Ok((nand, spare))
    }

    pub(super) fn read_single_block(&self, block_num: u32) -> Result<BlockSpare> {
        self.read_block_spare(block_num)
    }

    pub(super) fn write_single_block(
        &self,
        block: &[u8],
        spare: &[u8],
        block_num: u32,
    ) -> Result<()> {
        self.write_block_spare(block, spare, block_num)
    }
}
