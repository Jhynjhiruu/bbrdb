#![feature(duration_constants)]
#![feature(split_array)]
#![feature(let_chains)]

use chrono::prelude::*;
use commands::BlockSpare;
use std::mem::size_of;

use fs::FSBlock;
use rusb::{Device, DeviceHandle, DeviceList, Error, GlobalContext, Result};

pub(crate) mod commands;
mod fs;
mod player_comms;
mod usb;

#[derive(Debug)]
pub struct BBPlayer {
    handle: DeviceHandle<GlobalContext>,
    current_fs_index: u32,
    current_fs_block: Option<FSBlock>,
    current_fs_spare: Vec<u8>,
    is_initialised: bool,
}

trait FromBE {
    fn from_be_bytes(data: [u8; 4]) -> Self;
}

macro_rules! from_be {
    ($($t:ty)+) => {
        $(impl FromBE for $t {
            fn from_be_bytes(data: [u8; 4]) -> Self {
                Self::from_be_bytes(data)
            }
        })+
    };
}

from_be!(u32 i32);

macro_rules! check_initialised {
    ($e:expr, $b:block) => {
        if $e $b else { Err(Error::NoDevice) }
    };
}

fn num_from_arr<T: FromBE, U: AsRef<[u8]>>(data: U) -> T {
    assert!(data.as_ref().len() == size_of::<T>());
    T::from_be_bytes(*data.as_ref().split_array_ref().0)
}

impl BBPlayer {
    pub fn get_players() -> Result<Vec<Device<GlobalContext>>> {
        let devices = DeviceList::new()?;

        Ok(devices.iter().filter(Self::is_bbp).collect())
    }

    pub fn new(device: &Device<GlobalContext>) -> Result<Self> {
        Ok(Self {
            handle: Self::open_device(device)?,
            current_fs_index: 0,
            current_fs_block: None,
            current_fs_spare: vec![],
            is_initialised: false,
        })
    }

    #[allow(non_snake_case)]
    pub fn Init(&mut self) -> Result<()> {
        self.set_seqno(0x01)?;
        self.get_num_blocks()?;
        if !self.get_current_fs()? {
            return Err(Error::Io);
        }
        self.init_fs()?;
        self.delete_file_and_update("temp.tmp")?;
        self.is_initialised = true;
        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn GetBBID(&self) -> Result<u32> {
        check_initialised!(self.is_initialised, { self.get_bbid() })
    }

    #[allow(non_snake_case)]
    pub fn SetLED(&self, ledval: u32) -> Result<()> {
        check_initialised!(self.is_initialised, { self.set_led(ledval) })
    }

    // signhash

    #[allow(non_snake_case)]
    pub fn SetTime(&self) -> Result<()> {
        check_initialised!(self.is_initialised, {
            let now = Local::now();
            let timedata = [
                (now.year() % 100) as u8,
                now.month() as u8,
                now.day() as u8,
                now.weekday() as u8,
                0,
                now.hour() as u8,
                now.minute() as u8,
                now.second() as u8,
            ];

            self.set_time(timedata)
        })
    }

    #[allow(non_snake_case)]
    pub fn ListFileBlocks<T: AsRef<str>>(&self, filename: T) -> Result<Option<Vec<u16>>> {
        check_initialised!(self.is_initialised, {
            self.list_file_blocks(filename.as_ref())
        })
    }

    #[allow(non_snake_case)]
    pub fn ListFiles(&self) -> Result<Vec<(String, u32)>> {
        check_initialised!(self.is_initialised, { self.list_files() })
    }

    #[allow(non_snake_case)]
    pub fn DumpCurrentFS(&self) -> Result<()> {
        check_initialised!(self.is_initialised, { self.dump_current_fs() })
    }

    #[allow(non_snake_case)]
    pub fn DumpNAND(&self) -> Result<BlockSpare> {
        check_initialised!(self.is_initialised, { self.dump_nand_and_spare() })
    }

    #[allow(non_snake_case)]
    pub fn ReadSingleBlock(&self, block_num: u32) -> Result<BlockSpare> {
        check_initialised!(self.is_initialised, { self.read_single_block(block_num) })
    }

    // WriteNAND

    #[allow(non_snake_case)]
    pub fn WriteSingleBlock<T: AsRef<[u8]>, U: AsRef<[u8]>>(
        &self,
        block: T,
        spare: U,
        block_num: u32,
    ) -> Result<()> {
        check_initialised!(self.is_initialised, {
            self.write_single_block(block.as_ref(), spare.as_ref(), block_num)
        })
    }

    #[allow(non_snake_case)]
    pub fn ReadFile<T: AsRef<str>>(&self, filename: T) -> Result<Option<Vec<u8>>> {
        check_initialised!(self.is_initialised, { self.read_file(filename.as_ref()) })
    }

    // WriteFile

    #[allow(non_snake_case)]
    pub fn DeleteFile<T: AsRef<str>>(&mut self, filename: T) -> Result<()> {
        check_initialised!(self.is_initialised, { self.delete_file(filename.as_ref()) })
    }

    // PrintStats

    #[allow(non_snake_case)]
    pub fn Close(&mut self) -> Result<()> {
        check_initialised!(self.is_initialised, {
            match self.close_connection() {
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            self.is_initialised = false;
            Ok(())
        })
    }
}

impl Drop for BBPlayer {
    fn drop(&mut self) {
        if self.is_initialised {
            match self.close_connection() {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("{e}");
                    return;
                }
            }
            self.is_initialised = false;
        }
    }
}
