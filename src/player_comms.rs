use crate::{
    constants::{PACKET_SIZE, SEND_CHUNK_SIZE, TIMEOUT},
    error::{LibBBRDBError, Result},
    num_from_arr, BBPlayer,
};

#[repr(u8)]
pub(crate) enum TransferCommand {
    Ready = 0x15,

    PiecemealChunkRecv = 0x1C,

    PiecemealChunkSend = 0x40,
    Ack = 0x44,

    SendChunk = 0x63,
}

impl BBPlayer {
    const READY_SIGNAL: [u8; 4] = [TransferCommand::Ready as u8, 0x00, 0x00, 0x00];

    const PIECEMEAL_DATA_CHUNK_SIZE: usize = 3;

    pub fn send_chunked_data<T: AsRef<[u8]>>(&self, data: T) -> Result<()> {
        for chunk in data.as_ref().chunks(SEND_CHUNK_SIZE - 2) {
            let chunk_buf = [
                &[TransferCommand::SendChunk as u8, chunk.len() as u8],
                chunk,
            ]
            .concat();
            self.bulk_transfer_send(chunk_buf, TIMEOUT)?;
        }

        Ok(())
    }

    pub fn wait_ready(&self) -> Result<()> {
        while !self.is_ready()? {}
        Ok(())
    }

    fn is_ready(&self) -> Result<bool> {
        let buf = self.bulk_transfer_receive(4, TIMEOUT)?;
        if buf.len() != 4 {
            Err(LibBBRDBError::TransferLength(4, buf.len()))
        } else {
            Ok(buf == Self::READY_SIGNAL)
        }
    }

    fn encode_piecemeal_data(data: &[u8]) -> Vec<u8> {
        let mut rv = Vec::with_capacity(data.len() + (data.len() / 3) + (data.len() % 3).min(1));
        for chunk in data.chunks(Self::PIECEMEAL_DATA_CHUNK_SIZE) {
            rv.push(TransferCommand::PiecemealChunkSend as u8 + chunk.len() as u8);
            rv.extend(chunk);
        }
        rv
    }

    fn decode_piecemeal_data(data: &[u8], expected_len: usize) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(expected_len);
        let mut it = data.iter();
        while buf.len() < expected_len
            && let Some(&tu) = it.next()
        {
            match tu {
                0x1D..=0x1F => {
                    for i in TransferCommand::PiecemealChunkRecv as u8..tu {
                        buf.push(
                            *it.next()
                                .ok_or(LibBBRDBError::PiecemealChunkTooShort(tu, i))?,
                        );
                    }
                }
                _ => return Err(LibBBRDBError::UnexpectedPiecemealChunkType(tu)),
            }
        }
        assert!(
            buf.len() == expected_len,
            "Data length does not match expected"
        );
        Ok(buf)
    }

    pub fn send_piecemeal_data<T: AsRef<[u8]>>(&self, data: T) -> Result<usize> {
        self.bulk_transfer_send(Self::encode_piecemeal_data(data.as_ref()), TIMEOUT)
    }

    pub(crate) fn send_command(&self, command: u32, arg: u32) -> Result<()> {
        self.wait_ready()?;
        let message = [command.to_be_bytes(), arg.to_be_bytes()].concat();
        match self.send_piecemeal_data(message) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn send_ack(&self) -> Result<usize> {
        self.bulk_transfer_send([TransferCommand::Ack as u8], TIMEOUT)
    }

    fn receive_data_length(&self) -> Result<usize> {
        let mut data;
        loop {
            data = self.bulk_transfer_receive(4, TIMEOUT)?;
            if data == Self::READY_SIGNAL {
                eprintln!("Received unexpected ready signal");
                continue;
            }
            if data.len() != 4 || data[0] != 0x1B {
                return Err(LibBBRDBError::IncorrectDataLengthReply(
                    if !data.is_empty() {
                        Some(data[0])
                    } else {
                        None
                    },
                    data.len(),
                ));
            }
            break;
        }
        Ok((num_from_arr::<u32, _>(&data) & 0x00FFFFFF) as usize)
    }

    fn receive_data(&self, expected_len: usize) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(
            expected_len + (expected_len / 3) + (3 - (expected_len % 3)) % 3 + 1,
        );
        let mut transferred = PACKET_SIZE;

        while transferred == PACKET_SIZE {
            let mut recv =
                self.bulk_transfer_receive(PACKET_SIZE.min(buf.capacity() - buf.len()), TIMEOUT)?;
            transferred = recv.len();
            buf.append(&mut recv);
        }
        self.send_ack()?;
        Self::decode_piecemeal_data(&buf, expected_len)
    }

    pub fn receive_reply(&self, expected_len: usize) -> Result<Vec<u8>> {
        let data_length = self.receive_data_length()?;
        if data_length == 0 || data_length > expected_len {
            Err(LibBBRDBError::InvalidReplyLength(expected_len, data_length))
        } else {
            self.receive_data(data_length)
        }
    }

    pub fn receive_unknown_reply(&self) -> Result<Vec<u8>> {
        let data_length = self.receive_data_length()?;
        self.receive_data(data_length)
    }
}
