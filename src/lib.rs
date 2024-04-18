#![feature(duration_constants)]
#![feature(split_array)]
#![feature(let_chains)]

use chrono::prelude::*;
use commands::BlockSpare;
use num_traits::ToBytes;
use rdb::RDBPacketType;
use std::mem::size_of;

use error::{LibBBRDBError, Result};
use fs::FSBlock;
use rusb::{Device, DeviceHandle, DeviceList, GlobalContext};

use crate::{commands::Command, constants::TIMEOUT};

pub(crate) mod commands;
pub(crate) mod constants;
pub mod error;
mod fs;
mod player_comms;
pub mod rdb;
mod usb;

#[derive(Debug)]
pub struct BBPlayer {
    handle: DeviceHandle<GlobalContext>,
    current_fs_index: u32,
    current_fs_block: Option<Vec<FSBlock>>,
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
        if $e $b else { Err(LibBBRDBError::NoConsole) }
    };
}

fn num_from_arr<T: FromBE, U: AsRef<[u8]>>(data: U) -> T {
    assert!(data.as_ref().len() == size_of::<T>());
    match data.as_ref() {
        &[b0, b1, b2, b3] => T::from_be_bytes([b0, b1, b2, b3]),
        _ => unreachable!(),
    }
}

impl BBPlayer {
    pub fn get_players() -> Result<Vec<Device<GlobalContext>>> {
        let devices = DeviceList::new()?;
        let mut rv = vec![];

        for device in devices.iter() {
            if Self::is_bbp(&device)? {
                rv.push(device);
            }
        }

        Ok(rv)
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

    pub fn initialised(&self) -> bool {
        self.is_initialised
    }

    pub fn mux(&self) -> Result<()> {
        loop {
            let (cmd, data) = match self.receive_rdb() {
                Ok(c) => c,
                Err(LibBBRDBError::LibUSBError(rusb::Error::Timeout)) => break,
                x => x?,
            };

            self.bulk_transfer_send([68], TIMEOUT)?;
            /*match cmd {
                RDBPacketType::DevicePrint => todo!(),
                RDBPacketType::DeviceFault => todo!(),
                RDBPacketType::DeviceLogCT => todo!(),
                RDBPacketType::DeviceLog => todo!(),
                RDBPacketType::DeviceReadyForData => {
                    println!("ready");
                }
                RDBPacketType::DeviceDataCT => todo!(),
                RDBPacketType::DeviceData => todo!(),
                RDBPacketType::DeviceDebug => todo!(),
                RDBPacketType::DeviceRamRom => todo!(),
                RDBPacketType::DeviceDebugDone => todo!(),
                RDBPacketType::DeviceDebugReady => todo!(),
                RDBPacketType::DeviceKDebug => todo!(),
                RDBPacketType::DeviceProfData => todo!(),
                RDBPacketType::DeviceDataB => todo!(),
                RDBPacketType::DeviceSync => todo!(),

                _ => unreachable!(),
            }*/
            println!("{cmd:?}, {data:02X?}");
        }

        //let message = [&[8][..], &(2 as u32).to_be_bytes(), &0u32.to_be_bytes()].concat();
        //self.send_rdb(RDBPacketType::HostDataB, 1, &message)?;

        self.send_piecemeal_data([0, 0, 0, 2, 0, 0, 0, 0])?;

        //let message = [].concat();
        //self.bulk_transfer_send(&message,TIMEOUT)?;
        /*let data = [
            (16 << 2) | 3,
            0,
            0,
            0,
            (16 << 2) | 3,
            2,
            0,
            0,
            (16 << 2) | 2,
            0,
            0,
        ];
        self.bulk_transfer_send(data, TIMEOUT)?;*/

        //self.send_rdb(RDBPacketType::HostDataDone, 0, &[])?;

        let (cmd, data) = self.receive_rdb()?;
        println!("{cmd:?}, {data:02X?}");

        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn Init(&mut self) -> Result<()> {
        #[cfg(feature = "writing")]
        self.set_seqno(0x01)?;
        self.get_num_blocks()?;
        #[cfg(not(feature = "raw_access"))]
        if !self.get_current_fs()? {
            return Err(LibBBRDBError::FS);
        }
        #[cfg(not(feature = "raw_access"))]
        self.init_fs()?;
        #[cfg(all(feature = "writing", not(feature = "raw_access")))]
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
    pub fn SetTime<Tz: TimeZone>(&self, when: DateTime<Tz>) -> Result<()> {
        check_initialised!(self.is_initialised, {
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

            self.set_time(timedata)
        })
    }

    #[allow(non_snake_case)]
    #[cfg(not(feature = "raw_access"))]
    pub fn ListFileBlocks<T: AsRef<str>>(&self, filename: T) -> Result<Option<Vec<u16>>> {
        check_initialised!(self.is_initialised, {
            self.list_file_blocks(filename.as_ref())
        })
    }

    #[allow(non_snake_case)]
    #[cfg(not(feature = "raw_access"))]
    pub fn ListFiles(&self) -> Result<Vec<(String, u32)>> {
        check_initialised!(self.is_initialised, { self.list_files() })
    }

    #[allow(non_snake_case)]
    #[cfg(not(feature = "raw_access"))]
    pub fn DumpCurrentFS(&self) -> Result<Vec<u8>> {
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
    #[cfg(feature = "writing")]
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
    #[cfg(not(feature = "raw_access"))]
    pub fn ReadFile<T: AsRef<str>>(&self, filename: T) -> Result<Option<Vec<u8>>> {
        check_initialised!(self.is_initialised, { self.read_file(filename.as_ref()) })
    }

    #[allow(non_snake_case)]
    #[cfg(all(feature = "writing", not(feature = "raw_access")))]
    pub fn WriteFile<T: AsRef<[u8]>, U: AsRef<str>>(&mut self, data: T, filename: U) -> Result<()> {
        check_initialised!(self.is_initialised, {
            self.write_file(data.as_ref(), filename.as_ref())
        })
    }

    #[allow(non_snake_case)]
    #[cfg(all(feature = "writing", not(feature = "raw_access")))]
    pub fn DeleteFile<T: AsRef<str>>(&mut self, filename: T) -> Result<()> {
        check_initialised!(self.is_initialised, {
            self.delete_file_and_update(filename.as_ref())
        })
    }

    #[allow(non_snake_case)]
    #[cfg(not(feature = "raw_access"))]
    pub fn GetStats(&self) -> Result<(usize, usize, usize, u32)> {
        check_initialised!(self.is_initialised, { self.get_stats() })
    }

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

    #[allow(non_snake_case)]
    #[cfg(feature = "patched")]
    pub fn DumpV2(&mut self) -> Result<()> {
        check_initialised!(self.is_initialised, { self.dump_v2() })
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

#[cfg(test)]
mod tests {
    use std::fs::{read, write};

    use super::BBPlayer;
    use super::Result;
    use chrono::Local;

    #[test]
    fn it_works() -> Result<()> {
        let players = BBPlayer::get_players()?;
        println!("{players:#?}");
        let mut player = BBPlayer::new(&players[0])?;
        println!("{player:?}");
        player.Init()?;
        println!("{:04X}", player.GetBBID()?);
        player.SetLED(4)?;
        player.SetTime(Local::now())?;
        #[cfg(not(feature = "raw_access"))]
        {
            let blocks = match player.ListFileBlocks("hackit.sys")? {
                Some(b) => b,
                None => {
                    eprintln!("File not found");
                    vec![]
                }
            };
            println!("{blocks:X?}");
            let files = player.ListFiles()?;
            for file in files {
                println!("{:>12}: {}", file.0, file.1);
            }
            write("current_fs.bin", player.DumpCurrentFS()?).unwrap();
        }
        /*let (nand, spare) = player.DumpNAND()?;
        write("nand.bin", nand).unwrap();
        write("spare.bin", spare).unwrap();*/
        let (block, spare) = player.ReadSingleBlock(0)?;

        #[cfg(all(feature = "writing", not(feature = "raw_access")))]
        {
            write("block0.bin", &block).unwrap();
            write("spare0.bin", &spare).unwrap();
            player.WriteSingleBlock(block, spare, 0)?;
            /*let file = match player.ReadFile("00bbc0de.rec")? {
                Some(b) => b,
                None => {
                    eprintln!("File not found");
                    vec![]
                }
            };
            write("00bbc0de.rec", file).unwrap();*/
            let file = read("current_fs.bin").unwrap();
            player.WriteFile(&file, "test")?;
            player.WriteFile(&file, "testfile.bin")?;
            player.DeleteFile("testfile.bin")?;
            player.DeleteFile("test")?;
        }

        #[cfg(not(feature = "raw_access"))]
        {
            let (free, used, bad, seqno) = player.GetStats()?;
            println!("Free: {free} (0x{free:04X})\nUsed: {used} (0x{used:04X})\nBad: {bad} (0x{bad:04X})\nSequence Number: {seqno}");
        }

        Ok(())
    }
}
