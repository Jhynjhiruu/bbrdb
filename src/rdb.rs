use std::collections::VecDeque;
use std::mem::size_of;
use std::time::Duration;

use rusb::UsbContext;

use crate::constants::{RDB_BLOCKS_PER_CHUNK, RDB_BLOCK_SIZE, TIMEOUT};
use crate::error::*;
use crate::Handle;
use crate::LibBBRDBError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RDBCommand {
    DevicePrint = 1,
    DeviceFault,
    DeviceLogCT,
    DeviceLog,
    DeviceReadyForData,
    DeviceDataCT,
    DeviceData,
    DeviceDebug,
    DeviceRamRom,
    DeviceDebugDone,
    DeviceDebugReady,
    DeviceKDebug,
    DeviceProfData = 22,
    DeviceDataB = 23,
    DeviceSync = 25,

    HostLogDone = 13,
    HostDebug,
    HostDebugCT,
    HostData,
    HostDataDone,
    HostReqRamRom,
    HostFreeRamRom,
    HostKDebug,
    HostProfSignal,
    HostDataB = 24,
    HostSyncDone = 26,
    HostDebugDone,
}

impl TryFrom<u8> for RDBCommand {
    type Error = u8;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::DevicePrint),
            2 => Ok(Self::DeviceFault),
            3 => Ok(Self::DeviceLogCT),
            4 => Ok(Self::DeviceLog),
            5 => Ok(Self::DeviceReadyForData),
            6 => Ok(Self::DeviceDataCT),
            7 => Ok(Self::DeviceData),
            8 => Ok(Self::DeviceDebug),
            9 => Ok(Self::DeviceRamRom),
            10 => Ok(Self::DeviceDebugDone),
            11 => Ok(Self::DeviceDebugReady),
            12 => Ok(Self::DeviceKDebug),
            13 => Ok(Self::DeviceProfData),
            14 => Ok(Self::DeviceDataB),
            15 => Ok(Self::DeviceSync),

            16 => Ok(Self::HostLogDone),
            17 => Ok(Self::HostDebug),
            18 => Ok(Self::HostDebugCT),
            19 => Ok(Self::HostData),
            20 => Ok(Self::HostDataDone),
            21 => Ok(Self::HostReqRamRom),
            22 => Ok(Self::HostFreeRamRom),
            23 => Ok(Self::HostKDebug),
            24 => Ok(Self::HostProfSignal),
            25 => Ok(Self::HostDataB),
            26 => Ok(Self::HostSyncDone),
            27 => Ok(Self::HostDebugDone),

            _ => Err(value),
        }
    }
}

fn encode_rdb_hdr(cmd: RDBCommand, len: usize) -> u8 {
    ((cmd as u8) << 2) | (len as u8)
}

pub(crate) fn encode_rdb_packet(cmd: RDBCommand, data: &[u8]) -> Vec<u8> {
    let len = data.len();
    assert!(len < 4);

    let mut rv = vec![];

    rv.push(encode_rdb_hdr(cmd, len));
    rv.extend(data);

    rv
}

pub(crate) fn encode_rdb_block_packet(cmd: RDBCommand, data: &[u8]) -> Vec<u8> {
    let len = data.len();
    assert!(len <= RDB_BLOCK_SIZE);

    let mut rv = vec![];

    rv.push(encode_rdb_hdr(cmd, 0));
    rv.push(len as u8);
    rv.extend(data);

    rv
}

fn decode_rdb_cmd_len(byte: u8) -> Result<(RDBCommand, u8)> {
    let cmd = (byte >> 2).try_into();
    cmd.map(|c| (c, byte & 0b11))
        .map_err(LibBBRDBError::RDBUnknown)
}

fn to_u32(data: &[u8]) -> u32 {
    let mut v = vec![0; size_of::<u32>()];
    v.extend(data);
    u32::from_be_bytes(v[v.len() - 4..].try_into().unwrap())
}

impl<C: UsbContext> Handle<C> {
    fn send_rdb_block_data(&self, data: &[u8]) -> Result<()> {
        let cmd = RDBCommand::HostDataB;

        //println!("block send: {data:02X?}");

        for chunk in data.chunks(RDB_BLOCK_SIZE * RDB_BLOCKS_PER_CHUNK) {
            let mut buf = Vec::with_capacity(RDB_BLOCK_SIZE * RDB_BLOCKS_PER_CHUNK);
            for block in chunk.chunks(RDB_BLOCK_SIZE) {
                buf.extend(encode_rdb_block_packet(cmd, block));
            }

            if self.bulk_transfer_send(&buf, TIMEOUT)? != buf.len() {
                return Err(LibBBRDBError::WrongDataLength);
            }
        }

        Ok(())
    }

    fn send_rdb_data(&self, cmd: RDBCommand, data: &[u8]) -> Result<()> {
        //println!("send: {data:02X?}");

        for chunk in data.chunks(RDB_BLOCKS_PER_CHUNK) {
            let mut buf = Vec::with_capacity((chunk.len() * 4) / 3);
            for block in chunk.chunks(3) {
                buf.extend(encode_rdb_packet(cmd, block));
            }

            if self.bulk_transfer_send(&buf, TIMEOUT)? != buf.len() {
                return Err(LibBBRDBError::WrongDataLength);
            }
        }

        Ok(())
    }

    pub(crate) fn send_rdb_packets(&self, cmd: RDBCommand, data: &[u8]) -> Result<()> {
        for chunk in data.chunks(RDB_BLOCK_SIZE * RDB_BLOCKS_PER_CHUNK) {
            let mut buf = Vec::with_capacity(RDB_BLOCK_SIZE * RDB_BLOCKS_PER_CHUNK);
            for packet in chunk.chunks(RDB_BLOCK_SIZE) {
                if packet.len() < 4 {
                    buf.extend(encode_rdb_packet(cmd, packet));
                } else {
                    buf.extend(encode_rdb_block_packet(cmd, packet));
                }
            }

            //println!("raw: {:02X?}", buf);

            if self.bulk_transfer_send(&buf, TIMEOUT)? != buf.len() {
                return Err(LibBBRDBError::WrongDataLength);
            }
        }

        Ok(())
    }

    pub(crate) fn write_data<T: AsRef<[u8]>>(&self, cmd: RDBCommand, data: T) -> Result<()> {
        self.check_player_ready()?;

        let data = data.as_ref();

        if data.len() > 16 && cmd == RDBCommand::HostData {
            self.send_rdb_block_data(data)
        } else {
            self.send_rdb_data(cmd, data)
        }
    }

    pub(crate) fn read_rdb_packet(&self, timeout: Duration) -> Result<(RDBCommand, Vec<u8>)> {
        let data = self.bulk_transfer_receive(1, timeout)?[0];
        //println!("rdb packet: {:02X} {}", data >> 2, data & 3);
        let (cmd, len) = decode_rdb_cmd_len(data)?;
        if cmd == RDBCommand::DeviceDataB {
            let len = self.bulk_transfer_receive(1, timeout)?[0];

            Ok((cmd, self.bulk_transfer_receive(len as usize, timeout)?))
        } else {
            let mut data = self.bulk_transfer_receive(3, timeout)?;

            data.truncate(len as usize);

            Ok((cmd, data))
        }
    }

    pub(crate) fn read_rdb_bulk(&self, len: usize) -> Result<Vec<u8>> {
        let amount_to_read = ((len + 2) / 3) * 4;

        let data = self.bulk_transfer_receive(amount_to_read, TIMEOUT)?;

        let mut rv = vec![];

        for chunk in data.chunks(4) {
            let (cmd, len) = decode_rdb_cmd_len(chunk[0])?;
            assert_eq!(cmd, RDBCommand::DeviceData);

            rv.extend(&chunk[1..len as usize + 1]);
        }

        Ok(rv)
    }

    pub(crate) fn check_player_ready(&self) -> Result<bool> {
        self.read_rdb_packet(TIMEOUT)
            .map(|d| d.0 == RDBCommand::DeviceReadyForData)
    }

    fn send_ack(&self) -> Result<()> {
        self.bulk_transfer_send(&encode_rdb_packet(RDBCommand::HostDataDone, &[]), TIMEOUT)?;
        Ok(())
    }

    pub(crate) fn read_chunk(&self) -> Result<Vec<u8>> {
        let mut rv = vec![];

        let (cmd, data) = self.read_rdb_packet(TIMEOUT)?;
        if cmd != RDBCommand::DeviceDataCT {
            return Err(LibBBRDBError::RDBUnexpected(
                cmd,
                vec![RDBCommand::DeviceDataCT],
            ));
        }

        let count = to_u32(&data);
        //println!("count: {count:08X}");

        /*while rv.len() < count as usize {
            let (cmd, data) = self.read_rdb_packet()?;
            //println!("{cmd:?}");
            match cmd {
                RDBCommand::DeviceData => rv.extend(data),

                x => return Err(LibBBRDBError::RDBUnhandled(x)),
            }
        }*/

        rv = self.read_rdb_bulk(count as usize)?;

        self.send_ack()?;

        //println!("recv {rv:02X?}");

        Ok(rv)
    }

    pub(crate) fn read_data(&self, len: usize) -> Result<Vec<u8>> {
        let mut rv = vec![];

        while rv.len() < len {
            rv.extend(self.read_chunk()?);
        }

        Ok(rv)
    }
}
