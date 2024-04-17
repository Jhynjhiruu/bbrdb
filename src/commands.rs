use std::ffi::CString;

use crate::{
    constants::{BLOCK_CHUNK_SIZE, BLOCK_SIZE, SPARE_SIZE},
    error::{LibBBRDBError, Result},
    num_from_arr, BBPlayer,
};

use indicatif::ProgressIterator;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum Command {
    #[cfg(feature = "writing")]
    WriteBlock = 0x06,
    ReadBlock = 0x07,

    #[cfg(feature = "writing")]
    WriteBlockAndSpare = 0x10,
    ReadBlockAndSpare = 0x11,
    InitFS = 0x12,

    GetNumBlocks = 0x15,
    #[cfg(feature = "writing")]
    SetSeqNo = 0x16,
    GetSeqNo = 0x17,

    FileChksum = 0x1C,
    SetLED = 0x1D,
    SetTime = 0x1E,
    GetBBID = 0x1F,
    SignHash = 0x20,

    #[cfg(feature = "patched")]
    DumpV2 = 0x21,
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
    fn command_ret(buf: &[u8]) -> i32 {
        num_from_arr(&buf[4..8])
    }

    pub(super) fn read_string(&self) -> Result<()> {
        //self.wait_ready()?;
        println!("repl: {:02X?}", self.receive_unknown_reply()?);
        Ok(())
    }

    pub(super) fn read_block_spare(&self, block_num: u32) -> Result<BlockSpare> {
        // attempts
        for _ in 0..5 {
            self.request_block_read(Command::ReadBlockAndSpare, block_num)?;
            let block = try_continue!(self.get_block());
            let spare = try_continue!(self.get_spare());
            return Ok((block, spare));
        }
        Err(LibBBRDBError::ReadBlock(block_num))
    }

    fn request_block_read(&self, command: Command, block_num: u32) -> Result<()> {
        self.send_command(command as u32, block_num)?;
        let ret = Self::command_ret(&self.receive_reply(8)?);
        if ret < 0 {
            Err(LibBBRDBError::Command(command, ret))
        } else {
            Ok(())
        }
    }

    fn get_block(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(BLOCK_SIZE);
        for _ in 0..(BLOCK_SIZE / BLOCK_CHUNK_SIZE) {
            buf.extend(self.receive_reply(BLOCK_CHUNK_SIZE)?);
        }
        Ok(buf)
    }

    fn get_spare(&self) -> Result<Vec<u8>> {
        self.receive_reply(SPARE_SIZE)
    }

    #[cfg(feature = "writing")]
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
        Err(LibBBRDBError::WriteBlock(block_num))
    }

    #[cfg(feature = "writing")]
    fn request_block_write(&self, command: Command, block_num: u32) -> Result<()> {
        self.send_command(command as u32, block_num)?;
        self.wait_ready()
    }

    #[cfg(feature = "writing")]
    fn check_block_write(&self) -> Result<()> {
        let ret = Self::command_ret(&self.receive_reply(8)?);
        if ret < 0 {
            Err(LibBBRDBError::CheckBlockWrite(ret))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "writing")]
    fn send_block(&self, data: &[u8]) -> Result<()> {
        self.send_chunked_data(data)
    }

    #[cfg(feature = "writing")]
    fn send_spare(&self, data: &[u8]) -> Result<()> {
        self.wait_ready()?;
        let data = [&data[..3], &[0xFF; SPARE_SIZE - 3]].concat();
        match self.send_piecemeal_data(data) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub(super) fn init_fs(&self) -> Result<()> {
        self.send_command(Command::InitFS as u32, 0x00)?;
        let ret = Self::command_ret(&self.receive_reply(8)?);
        if ret < 0 {
            Err(LibBBRDBError::InitFS(ret))
        } else {
            Ok(())
        }
    }

    pub(super) fn get_num_blocks(&self) -> Result<u32> {
        self.send_command(Command::GetNumBlocks as u32, 0x00)?;
        let reply = self.receive_reply(8)?;
        let size: u32 = num_from_arr(&reply[4..8]);
        Ok(size)
    }

    #[cfg(feature = "writing")]
    pub(super) fn set_seqno(&self, arg: u32) -> Result<()> {
        self.send_command(Command::SetSeqNo as u32, arg)?;
        self.receive_reply(8)?;
        Ok(())
    }

    pub(super) fn file_checksum_cmp(&self, filename: &str, chksum: u32, size: u32) -> Result<bool> {
        self.send_filename(filename)?;
        self.send_params_and_receive_reply(chksum, size)
    }

    fn send_filename(&self, filename: &str) -> Result<()> {
        let split = filename.split('.').collect::<Vec<_>>();
        let (name, ext) = if split.len() > 1 {
            (split[0], split[1])
        } else {
            (split[0], "")
        };

        if name.len() > 8 || ext.len() > 3 {
            return Err(LibBBRDBError::FileNameTooLong(filename.to_string()));
        }

        let send_buf = match CString::new(filename) {
            Ok(f) => f,
            Err(_) => return Err(LibBBRDBError::FileNameCString(filename.to_string())),
        };

        self.send_command(
            Command::FileChksum as u32,
            send_buf.as_bytes_with_nul().len() as u32,
        )?;

        self.wait_ready()?;

        self.send_piecemeal_data(
            [
                send_buf.as_bytes_with_nul(),
                &vec![0x00u8; (4 - (send_buf.as_bytes_with_nul().len() % 4)) % 4],
            ]
            .concat(),
        )?;

        //self.wait_ready()
        Ok(())
    }

    fn send_params_and_receive_reply(&self, chksum: u32, size: u32) -> Result<bool> {
        self.send_command(chksum, size)?;
        //self.wait_ready()?;
        let reply = self.receive_reply(8)?;
        Ok(num_from_arr::<i32, _>(&reply[4..8]) == 0)
    }

    pub(super) fn set_led(&self, ledval: u32) -> Result<()> {
        self.send_command(Command::SetLED as u32, ledval)?;
        self.receive_reply(8)?;
        Ok(())
    }

    pub(super) fn set_time(&self, timedata: [u8; 8]) -> Result<()> {
        let first_half = num_from_arr(*timedata.split_array_ref::<4>().0);
        let second_half = &timedata[4..];
        self.send_command(Command::SetTime as u32, first_half)?;
        let ret = Self::command_ret(&self.receive_reply(8)?);
        if ret < 0 {
            Err(LibBBRDBError::SetTime(ret))
        } else {
            self.send_piecemeal_data(second_half)?;
            Ok(())
        }
    }

    pub(super) fn get_bbid(&self) -> Result<u32> {
        self.send_command(Command::GetBBID as u32, 0x00)?;
        let reply = self.receive_reply(8)?;
        let ret = Self::command_ret(&reply);
        if ret < 0 {
            Err(LibBBRDBError::GetBBID(ret))
        } else {
            Ok(num_from_arr(&reply[4..8]))
        }
    }

    pub(super) fn dump_nand_and_spare(&self) -> Result<BlockSpare> {
        let num_blocks = self.get_num_blocks()?;
        let mut nand = Vec::with_capacity(num_blocks as usize * BLOCK_SIZE);
        let mut spare = Vec::with_capacity(num_blocks as usize * SPARE_SIZE);
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

    #[cfg(feature = "writing")]
    pub(super) fn write_single_block(
        &self,
        block: &[u8],
        spare: &[u8],
        block_num: u32,
    ) -> Result<()> {
        self.write_block_spare(block, spare, block_num)
    }

    #[cfg(feature = "patched")]
    pub(super) fn dump_v2(&mut self) -> Result<()> {
        use std::fs::write;

        self.send_command(Command::DumpV2 as u32, 0x00)?;
        let ret = Self::command_ret(&self.receive_reply(8)?);
        if ret < 0 {
            return Err(LibBBRDBError::Command(Command::DumpV2, ret));
        }
        let buf = self.receive_reply(0x100)?;
        write("v2.bin", buf).unwrap();
        Ok(())
    }
}
