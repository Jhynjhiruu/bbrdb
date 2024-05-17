use core::num;
use std::mem::size_of;

use rusb::UsbContext;

use crate::constants::STATUS_OFFSET;
use crate::error::*;
use crate::fs::Fat;
use crate::rdb::RDBCommand;
use crate::Handle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Command {
    Ping = 0x01,
    PowerOff = 0x02,

    WriteBlock = 0x06,
    ReadBlock = 0x07,

    ReadDir = 0x08,
    WriteFile = 0x09,
    ReadFile = 0x0A,
    DeleteFile = 0x0B,

    ScanBlocks = 0x0D,

    RenameFile = 0x0F,

    WriteBlockAndSpare = 0x10,
    ReadBlockAndSpare = 0x11,
    InitFS = 0x12,

    SumFile = 0x13,

    FreeBlocks = 0x14,

    GetNumBlocks = 0x15,
    SetSeqNo = 0x16,
    GetSeqNo = 0x17,

    StatFile = 0x18,

    ReadFileBlock = 0x19,
    WriteFileBlock = 0x1A,

    CreateFile = 0x1B,

    ChksumFile = 0x1C,
    SetLED = 0x1D,
    SetTime = 0x1E,
    GetBBID = 0x1F,
    SignHash = 0x20,
}

pub trait CommandArgs {
    fn encode(self) -> Vec<u8>;
}

impl CommandArgs for u32 {
    fn encode(self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }
}

impl CommandArgs for Vec<u8> {
    fn encode(self) -> Vec<u8> {
        self
    }
}

impl CommandArgs for &[u8] {
    fn encode(self) -> Vec<u8> {
        self.to_vec()
    }
}

impl<C: UsbContext> Handle<C> {
    fn write_data_len(&self, data: &[u8]) -> Result<()> {
        let len = data.len();
        assert!(len <= i32::MAX as usize);

        self.write_data(RDBCommand::HostData, (len as i32).to_be_bytes())?;

        self.write_data(RDBCommand::HostData, data)
    }

    pub(crate) fn send_data<T: AsRef<[u8]>>(&self, data: T) -> Result<()> {
        self.write_data_len(data.as_ref())
    }

    pub(crate) fn send_command<T: CommandArgs>(&self, command: Command, args: T) -> Result<()> {
        let mut data = vec![];

        data.extend((command as u32).to_be_bytes());
        data.extend(args.encode());

        self.write_data(RDBCommand::HostData, &data)
    }

    fn get_response(&self, len: usize) -> Result<Vec<u32>> {
        self.read_data(len).map(|d| {
            d.chunks(size_of::<u32>())
                .map(|c| u32::from_be_bytes(c.try_into().unwrap()))
                .collect()
        })
    }

    pub(crate) fn check_cmd_response(&self, command: Command, len: usize) -> Result<Vec<u32>> {
        let data = self.get_response((len + 1) * size_of::<u32>())?;
        let c = data.first().map(u32::to_owned).unwrap_or_default();
        if c != 255 - command as u32 {
            return Err(LibBBRDBError::IncorrectCmdResponse(c, 255 - command as u32));
        }

        Ok(data[1..].to_vec())
    }

    pub(crate) fn command_response<T: CommandArgs>(
        &self,
        command: Command,
        args: T,
        len: usize,
    ) -> Result<Vec<u32>> {
        self.send_command(command, args)?;
        self.check_cmd_response(command, len)
    }

    pub(crate) fn read_blocks(&self, block: u32, num_blocks: u32) -> Result<Vec<u8>> {
        let mut rv = vec![];

        for blk in block..block + num_blocks {
            let status = self.command_response(Command::ReadBlock, blk, 1)?[0];
            rv.extend(self.read_data(0x4000)?);

            if status != 0 {
                return Err(CardError::from_u32(status).into());
            }
        }

        Ok(rv)
    }

    pub(crate) fn read_blocks_spare(
        &self,
        block: u32,
        num_blocks: u32,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut nand = vec![];
        let mut spare = vec![];

        for blk in block..block + num_blocks {
            let status = self.command_response(Command::ReadBlockAndSpare, blk, 1)?[0];
            let n = self.read_data(0x4000)?;
            let s = self.read_data(0x10)?;

            if s[STATUS_OFFSET].count_zeros() > 1 {
                return Err(CardError::BadBlock(n, s).into());
            }

            if status != 0 {
                return Err(CardError::from_u32(status).into());
            }

            nand.extend(n);
            spare.extend(s);
        }

        Ok((nand, spare))
    }

    pub(crate) fn write_blocks(&mut self, block: u32, data: &[&[u8]]) -> Result<()> {
        for (index, nand) in data.iter().enumerate() {
            let index = index as u32;

            let blk = block + index;

            self.send_command(Command::WriteBlock, blk)?;
            self.write_data(RDBCommand::HostData, nand)?;

            let status = self.check_cmd_response(Command::WriteBlock, 1)?[0];
            if status != 0 {
                return Err(CardError::from_u32(status).into());
            }
        }

        Ok(())
    }

    pub(crate) fn write_blocks_spare(&mut self, block: u32, data: &[(&[u8], &[u8])]) -> Result<()> {
        for (index, (nand, spare)) in data.iter().enumerate() {
            let index = index as u32;

            let blk = block + index;

            self.send_command(Command::WriteBlockAndSpare, blk)?;
            self.write_data(RDBCommand::HostData, nand)?;
            self.write_data(RDBCommand::HostData, spare)?;

            let status = self.check_cmd_response(Command::WriteBlockAndSpare, 1)?[0];
            if status != 0 {
                return Err(CardError::from_u32(status).into());
            }
        }

        Ok(())
    }

    #[allow(non_snake_case)]
    pub(crate) fn SetCardSeqno(&self) -> Result<Option<(Option<Fat>, u32)>> {
        let resp = self.command_response(Command::SetSeqNo, 1, 1)?;
        if resp[0] == 0 {
            return Ok(None);
        }

        let resp = self.command_response(Command::GetNumBlocks, 0, 1)?;

        let cardsize = if resp[0] % 4096 == 0 {
            resp[0]
        } else {
            return Err(LibBBRDBError::UnhandledCardSize);
        };

        match self.read_fat(cardsize) {
            Ok(f) => Ok(Some((Some(f), cardsize))),
            Err(LibBBRDBError::NoFAT) => Ok(Some((None, cardsize))),
            Err(e) => Err(e),
        }
    }
}
