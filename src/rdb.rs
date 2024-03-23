use crate::{constants::TIMEOUT, error::LibBBRDBError, num_from_arr, BBPlayer, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RDBPacketType {
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

impl TryFrom<u8> for RDBPacketType {
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

impl BBPlayer {
    pub fn receive_rdb(&self) -> Result<(RDBPacketType, Vec<u8>)> {
        let data = self.bulk_transfer_receive(4, TIMEOUT)?;
        if data.len() != 4 {
            return Err(LibBBRDBError::IncorrectDataLengthReply(
                if !data.is_empty() {
                    Some(data[0])
                } else {
                    None
                },
                data.len(),
            ));
        }

        let cmd = match ((data[0] >> 2) & 0x3F).try_into() {
            Ok(c) => c,
            Err(e) => return Err(LibBBRDBError::InvalidRDBCommand(e)),
        };

        let len = data[0] & 3;

        let data = data[1..len as usize + 1].to_vec();

        Ok((cmd, data))
    }

    pub fn send_rdb(&self, cmd: RDBPacketType, len: u8, data: &[u8]) -> Result<usize> {
        assert!(len < 4);
        let cmd = cmd as u8;
        let packet = (cmd << 2) | len;

        let message = [&[packet], data].concat();
        self.bulk_transfer_send(message, TIMEOUT)
    }
}
