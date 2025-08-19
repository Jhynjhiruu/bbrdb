use bb::CmdHead;
use bb::Spare;
use bb::SpareData;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;

use crate::error::*;
use crate::Handle;

impl Handle {
    fn skip_bad_blocks(&mut self, mut blk: u32, num_blocks: usize) -> Result<(Vec<u8>, Vec<u8>, u32)> {
        let mut nand = vec![];
        let mut spare = vec![];

        let mut blocks_read = 0;
        while blocks_read < num_blocks {
            match self.read_blocks_spare(blk, 1) {
                Ok((n, s)) => {
                    nand.extend(n);
                    spare.extend(s);
                    blocks_read += 1;
                }
                Err(e) => {
                    eprintln!("SK bad block ({blk}): {e}");
                }
            }
            blk += 1;
        }

        Ok((nand, spare, blk))
    }

    fn read_sk(&mut self) -> Result<(Vec<u8>, u32)> {
        let (rv, _, blk) = self.skip_bad_blocks(0, 4)?;

        if blk >= 8 {
            Err(LibBBRDBError::BadSKSA)
        } else {
            Ok((rv, blk))
        }
    }

    fn read_sa(&mut self, blk: u32) -> Result<(Vec<u8>, u32)> {
        let (mut rv, cmd_spare, _) = self.skip_bad_blocks(blk, 1)?;

        let cmd = CmdHead::read_from_buf(&rv[..CmdHead::SIZE])?;
        let cmd_spare: SpareData = Spare::read_from_buf(&cmd_spare)?.into();

        let mut blk = cmd_spare.sa_block as u32;

        let bar = ProgressBar::new(cmd.size as u64).with_style(
            ProgressStyle::with_template(
                "{wide_bar} {bytes}/{total_bytes}, eta {eta} ({binary_bytes_per_sec})",
            )
            .unwrap(),
        );
        let mut sa = vec![];
        while sa.len() < cmd.size as usize {
            let (block, spare) = self.read_blocks_spare(blk, 1)?;

            bar.inc(block.len() as u64);

            sa.extend(block);

            let spare: SpareData = Spare::read_from_buf(&spare)?.into();
            blk = spare.sa_block as u32;
        }

        rv.extend(sa);

        Ok((rv, blk))
    }

    #[allow(non_snake_case)]
    pub fn ReadSKSA(&mut self) -> Result<Vec<u8>> {
        let mut rv = vec![];

        let (sk, blk) = self.read_sk()?;
        rv.extend(sk);

        let (sa, blk) = self.read_sa(blk)?;
        rv.extend(sa);

        if blk != 0xFF {
            // sa2
            let (sa2, _) = self.read_sa(blk)?;
            rv.extend(sa2);
        }

        Ok(rv)
    }
}
