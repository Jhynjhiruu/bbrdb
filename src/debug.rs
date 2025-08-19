use std::time::Duration;

use crate::error::*;
use crate::rdb::RDBCommand;
use crate::Handle;

impl Handle {
    pub fn debug_send(&mut self, data: &[u8]) -> Result<()> {
        self.send_rdb_packets(RDBCommand::HostDebug, data)?;
        self.send_rdb_packets(RDBCommand::HostDebugDone, &[0])
    }

    pub fn debug_wait(&mut self) -> Result<Vec<u8>> {
        let mut resp = vec![];

        loop {
            let (cmd, data) = match self.read_rdb_packet(Duration::from_secs(1)) {
                Ok(r) => r,
                Err(LibBBRDBError::Timeout(_)) => continue,
                x => x?,
            };
            match cmd {
                RDBCommand::DeviceDebug => resp.extend(data),
                RDBCommand::DeviceDebugReady => break,
                _ => {
                    return Err(LibBBRDBError::RDBUnexpected(
                        cmd,
                        vec![RDBCommand::DeviceDebug, RDBCommand::DeviceDebugReady],
                    ));
                }
            }
        }

        Ok(resp)
    }

    pub fn wait_mux(&mut self) -> Result<String> {
        loop {
            let (cmd, data) = match self.read_rdb_packet(Duration::from_secs(1)) {
                Ok(r) => r,
                Err(LibBBRDBError::Timeout(_)) => continue,
                x => x?,
            };
            match cmd {
                RDBCommand::DevicePrint => return Ok("print\n".into()),
                _ => return Ok("not print\n".into()),
            }
        }

        Ok("\n".into())
    }
}
