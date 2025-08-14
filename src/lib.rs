use std::{iter::repeat, thread::sleep, time::Duration};

use chrono::{DateTime, Datelike, TimeZone, Timelike};
use commands::Command;
use constants::{BLOCK_SIZE, SPARE_SIZE};
use fs::Fat;
use indicatif::ProgressIterator;
use rdb::RDBCommand;
use rusb::{Device, DeviceHandle, DeviceList, GlobalContext, UsbContext};

mod commands;
mod constants;
mod debug;
mod error;
mod fs;
mod kernel;
mod rdb;
mod usb;

pub use error::*;
pub use fs::CardStats;
pub use usb::*;

#[derive(Debug)]
struct BBPlayer {
    fat: Option<Fat>,
    cardsize: u32,
}

impl BBPlayer {
    fn new<C: UsbContext>(handle: &mut Handle<C>) -> Result<Option<Self>> {
        let status = handle.SetCardSeqno()?;

        Ok(status.map(|(fat, cardsize)| Self { fat, cardsize }))
    }
}

#[derive(Debug)]
pub struct Handle<C: UsbContext> {
    handle: DeviceHandle<C>,
    device: Option<BBPlayer>,
}

#[macro_export]
macro_rules! require_init {
    ($s:expr, $p:ident $c:block) => {
        if !$s.initialised()? {
            return Err(LibBBRDBError::NotInitialised);
        } else {
            if let Some($p) = &$s.device {
                $c
            } else {
                Err(LibBBRDBError::NotInitialised)
            }
        }
    };
    (mut $s:expr, $p:ident $c:block) => {
        if !$s.initialised()? {
            return Err(LibBBRDBError::NotInitialised);
        } else {
            if let Some($p) = &mut $s.device {
                $c
            } else {
                Err(LibBBRDBError::NotInitialised)
            }
        }
    };
}

#[macro_export]
macro_rules! require_fat {
    ($s:expr, $p:ident, $f:ident $c:block) => {
        require_init!($s, $p {
            if let Some($f) = &$p.fat {
                $c
            } else {
                Err(LibBBRDBError::NoFAT)
            }
        })
    };
    (mut $s:expr, $p:ident, $f:ident $c:block) => {
        require_init!(mut $s, $p {
            if let Some($f) = &mut $p.fat {
                $c
            } else {
                Err(LibBBRDBError::NoFAT)
            }
        })
    };
}

impl<C: UsbContext> Handle<C> {
    pub fn new(device: &Device<C>) -> Result<Self> {
        Ok(Self {
            handle: open_device(device)?,
            device: None,
        })
    }

    pub fn initialised(&self) -> Result<bool> {
        Ok(self.device.is_some() && self.GetCardSeqno()?)
    }

    fn check_initialised(&self) -> Result<()> {
        if !self.initialised()? {
            Err(LibBBRDBError::NotInitialised)
        } else {
            Ok(())
        }
    }

    fn get_num_blocks(&self) -> Result<u32> {
        let resp = self.command_response(Command::GetNumBlocks, 0, 1)?[0];
        if resp < 0 {
            Err(CardError::from_i32(resp).into())
        } else {
            Ok(resp as u32)
        }
    }

    #[allow(non_snake_case)]
    pub fn Init(&mut self) -> Result<()> {
        if self.initialised()? {
            self.Close()?;
        }

        self.device = BBPlayer::new(self)?;

        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn SetLED(&mut self, ledval: u32) -> Result<()> {
        self.command_response(Command::SetLED, ledval, 1)?;
        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn SetTime<Tz: TimeZone>(&mut self, when: DateTime<Tz>) -> Result<()> {
        let timedata = [
            (when.year() % 100) as u8,
            when.month() as u8,
            when.day() as u8,
            when.weekday() as u8,
            0,
            when.hour() as u8,
            when.minute() as u8,
            when.second() as u8,
        ];

        let status = self.command_response(Command::SetTime, &timedata[..4], 1)?[0];
        if status < 0 {
            Err(LibBBRDBError::SetTime(status))
        } else {
            self.write_data(RDBCommand::HostData, &timedata[4..])?;
            Ok(())
        }
    }

    #[allow(non_snake_case)]
    pub fn GetBBID(&self) -> Result<u32> {
        Ok(self.command_response(Command::GetBBID, 0, 1)?[0] as u32)
    }

    #[allow(non_snake_case)]
    pub fn ScanBadBlocks(&self) -> Result<Vec<bool>> {
        let blocks = {
            let this = &self;
            let command = Command::ScanBlocks;
            this.send_command(command, 0)?;
            sleep(Duration::from_secs(10));
            this.check_cmd_response(command, 1)
        }?[0];
        let blocklist = self.read_data(blocks as usize)?;

        Ok(blocklist.into_iter().map(|b| b != 0).collect())
    }

    #[allow(non_snake_case)]
    pub fn DumpNAND(&self) -> Result<Vec<u8>> {
        let num_blocks = if self.initialised()? {
            let Some(player) = &self.device else {
                unreachable!()
            };
            player.cardsize
        } else {
            self.get_num_blocks()?
        };

        let mut nand = vec![];

        for i in (0..num_blocks).progress() {
            let blk = self.read_blocks(i, 1);
            match blk {
                Ok(b) => nand.extend(b),
                Err(e) => {
                    nand.extend(repeat(0).take(0x4000));
                    eprintln!("{e}");
                }
            }
        }

        Ok(nand)
    }

    #[allow(non_snake_case)]
    pub fn DumpNANDSpare(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let num_blocks = if self.initialised()? {
            let Some(player) = &self.device else {
                unreachable!()
            };
            player.cardsize
        } else {
            self.get_num_blocks()?
        };

        let mut nand = vec![];
        let mut spare = vec![];

        for i in (0..num_blocks).progress() {
            let blk = self.read_blocks_spare(i, 1);
            match blk {
                Ok((n, s)) => {
                    nand.extend(n);
                    spare.extend(s);
                }
                Err(LibBBRDBError::CardError(CardError::BadBlock(n, s))) => {
                    nand.extend(n);
                    spare.extend(s);
                    eprintln!("bad block: {i}");
                }
                Err(e) => {
                    nand.extend(repeat(0).take(0x4000));
                    spare.extend(repeat(0).take(0x10));
                    eprintln!("{e}");
                }
            }
        }

        Ok((nand, spare))
    }

    #[allow(non_snake_case)]
    pub fn WriteNAND(&mut self, nand: &[u8], which_blocks: Option<Vec<u16>>) -> Result<()> {
        let num_blocks = if self.initialised()? {
            let Some(player) = &self.device else {
                unreachable!()
            };
            player.cardsize
        } else {
            self.get_num_blocks()?
        };

        if nand.len() != num_blocks as usize * BLOCK_SIZE {
            return Err(LibBBRDBError::InvalidNANDSize(
                nand.len(),
                num_blocks as usize * BLOCK_SIZE,
            ));
        }

        for i in match which_blocks {
            Some(b) => b,
            None => (0..num_blocks as u16).collect(),
        }
        .into_iter()
        .progress()
        {
            match self.write_blocks(
                i as u32,
                &[&nand[i as usize * BLOCK_SIZE..(i as usize + 1) * BLOCK_SIZE]],
            ) {
                Ok(()) => {}
                Err(e) => eprintln!("{e}"),
            }
        }

        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn WriteNANDSpare(
        &mut self,
        nand: &[u8],
        spare: &[u8],
        which_blocks: Option<Vec<u16>>,
    ) -> Result<()> {
        let num_blocks = if self.initialised()? {
            let Some(player) = &self.device else {
                unreachable!()
            };
            player.cardsize
        } else {
            self.get_num_blocks()?
        };

        if nand.len() != num_blocks as usize * BLOCK_SIZE {
            return Err(LibBBRDBError::InvalidNANDSize(
                nand.len(),
                num_blocks as usize * BLOCK_SIZE,
            ));
        }

        if spare.len() != num_blocks as usize * SPARE_SIZE {
            return Err(LibBBRDBError::InvalidSpareSize(
                spare.len(),
                num_blocks as usize * SPARE_SIZE,
            ));
        }

        for i in match which_blocks {
            Some(b) => b,
            None => (0..num_blocks as u16).collect(),
        }
        .into_iter()
        .progress()
        {
            match self.write_blocks_spare(
                i as u32,
                &[(
                    &nand[i as usize * BLOCK_SIZE..(i as usize + 1) * BLOCK_SIZE],
                    &spare[i as usize * SPARE_SIZE..(i as usize + 1) * SPARE_SIZE],
                )],
            ) {
                Ok(()) => {}
                Err(e) => eprintln!("{e}"),
            }
        }

        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn ReadSingleBlock(&self, block_num: u32) -> Result<(Vec<u8>, Vec<u8>)> {
        self.read_blocks_spare(block_num, 1)
    }

    #[allow(non_snake_case)]
    pub fn WriteSingleBlock(&mut self, block_num: u32, data: &[u8], spare: &[u8]) -> Result<()> {
        self.write_blocks_spare(block_num, &[(data, spare)])
    }

    #[allow(non_snake_case)]
    pub fn Close(&mut self) -> Result<()> {
        self.check_initialised()?;

        self.device = None;

        Ok(())
    }
}
