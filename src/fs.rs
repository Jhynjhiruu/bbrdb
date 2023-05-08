use std::{
    ffi::CString,
    fs::write,
    io::{Cursor, Seek},
};

use crate::{
    constants::{BLOCK_SIZE, SPARE_SIZE},
    num_from_arr, BBPlayer,
};
use indicatif::{ProgressBar, ProgressStyle};
use rusb::{Error, Result};

use binrw::{binrw, BinReaderExt, BinResult, BinWriterExt};

#[binrw]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum FATEntry {
    #[brw(magic = 0x0000u16)]
    Free,
    #[brw(magic = 0xFFFFu16)]
    EndOfChain,
    #[brw(magic = 0xFFFEu16)]
    BadBlock,
    #[brw(magic = 0xFFFDu16)]
    Reserved,
    Chain(u16),
}

#[binrw]
#[derive(Debug, PartialEq)]
pub enum FileValid {
    #[brw(magic = 0x00u8)]
    Invalid,
    #[brw(magic = 0x01u8)]
    Valid,
}

#[binrw]
#[derive(Debug)]
pub struct FileEntry {
    name: [u8; 8],
    ext: [u8; 3],
    valid: FileValid,
    #[brw(pad_after(2))]
    start: FATEntry,
    size: u32,
}

#[binrw]
#[derive(Debug)]
pub enum FSType {
    #[brw(magic = b"BBFS")]
    Bbfs,
    #[brw(magic = b"BBFL")]
    Bbfl,
}

#[binrw]
#[derive(Debug)]
pub struct FSFooter {
    fs_type: FSType,
    seqno: u32,
    link_block: u16,
    chksum: u16,
}

#[binrw]
#[derive(Debug)]
pub(crate) struct FSBlock {
    fat: [FATEntry; 0x1000],
    entries: [FileEntry; 409],
    footer: FSFooter,
}

impl FSBlock {
    fn read<T: AsRef<[u8]>>(data: T) -> BinResult<Self> {
        let mut cursor = Cursor::new(data.as_ref());
        match <_>::read_be(&mut cursor) {
            Ok(fs) => {
                /*if data.as_ref().chunks(2).fold(0u16, |a, e| {
                    a.wrapping_add(u16::from_be_bytes(*e.split_array_ref().0))
                }) != 0xCAD7
                {
                    Err(binrw::Error::AssertFail {
                        pos: 0x3FFE,
                        message: "Invalid checksum".to_string(),
                    })
                } else */
                {
                    Ok(fs)
                }
            }
            Err(e) => Err(e),
        }
    }

    fn write(&self) -> BinResult<Vec<u8>> {
        let mut cursor = Cursor::new(vec![]);
        match cursor.write_be(self) {
            Ok(_) => {
                let data = cursor.into_inner();
                let sum = data[..0x3FFE].as_ref().chunks(2).fold(0u16, |a, e| {
                    a.wrapping_add(u16::from_be_bytes(*e.split_array_ref().0))
                });
                let checksum = 0xCAD7u16.wrapping_sub(sum);
                cursor = Cursor::new(data);
                cursor.seek(std::io::SeekFrom::End(-2)).unwrap();
                cursor.write_be(&checksum).unwrap();
                Ok(cursor.into_inner())
            }
            Err(e) => Err(e),
        }
    }
}

impl FileEntry {
    fn valid(&self) -> bool {
        self.name[0] != 0 && self.valid == FileValid::Valid && self.start != FATEntry::EndOfChain
    }

    fn set_filename(&mut self, filename: &str) -> Result<()> {
        let split = filename.split('.').collect::<Vec<_>>();
        let (name, ext) = if split.len() > 1 {
            (split[0], split[1])
        } else {
            (split[0], "")
        };

        if name.len() > 8 || ext.len() > 3 {
            return Err(Error::Overflow);
        }

        self.name
            .copy_from_slice((name.to_owned() + &"\0".repeat(8 - name.len())).as_bytes());
        self.ext
            .copy_from_slice((ext.to_owned() + &"\0".repeat(3 - ext.len())).as_bytes());

        Ok(())
    }

    fn get_filename(&self) -> String {
        match self.name.iter().enumerate().find(|(_, &e)| e == 0) {
            Some((index, _)) => CString::new(&self.name[..index]),
            None => CString::new(self.name),
        }
        .expect("Already checked for unexpected nulls")
        .to_string_lossy()
        .into_owned()
    }

    fn get_fileext(&self) -> String {
        match self.ext.iter().enumerate().find(|(_, &e)| e == 0) {
            Some((index, _)) => CString::new(&self.ext[..index]),
            None => CString::new(self.ext),
        }
        .expect("Already checked for unexpected nulls")
        .to_string_lossy()
        .into_owned()
    }

    fn get_fullname(&self) -> String {
        format!(
            "{}{}{}",
            self.get_filename(),
            if self.get_filename() != "" && self.get_fileext() != "" {
                "."
            } else {
                ""
            },
            self.get_fileext()
        )
    }

    fn clear(&mut self) {
        self.name = [0; 8];
        self.ext = [0; 3];
        self.valid = FileValid::Invalid;
        self.start = FATEntry::Free;
        self.size = 0;
    }
}

impl BBPlayer {
    fn get_file(&mut self, filename: &str) -> Result<Option<&mut FileEntry>> {
        if let Some(block) = &mut self.current_fs_block {
            for file in &mut block.entries {
                if file.valid() && file.get_fullname() == filename {
                    return Ok(Some(file));
                }
            }
            Ok(None)
        } else {
            Err(Error::NoDevice)
        }
    }

    fn find_file(&self, filename: &str) -> Result<Option<&FileEntry>> {
        if let Some(block) = &self.current_fs_block {
            for file in &block.entries {
                if file.valid() && file.get_fullname() == filename {
                    return Ok(Some(file));
                }
            }
            Ok(None)
        } else {
            Err(Error::NoDevice)
        }
    }

    fn rename_file(&mut self, from: &str, to: &str) -> Result<()> {
        match self.get_file(from)? {
            Some(f) => f.set_filename(to),
            None => Err(Error::InvalidParam),
        }
    }

    fn bytes_to_blocks(bytes: usize) -> usize {
        (bytes + BLOCK_SIZE - 1) / BLOCK_SIZE
    }

    fn get_file_block_count(&self, filename: &str) -> Result<usize> {
        if let Some(block) = &self.current_fs_block {
            match self.find_file(filename)? {
                Some(f) => Ok(Self::bytes_to_blocks(f.size as usize)),
                None => Err(Error::InvalidParam),
            }
        } else {
            Err(Error::NoDevice)
        }
    }

    fn get_free_block_count(&self) -> Result<usize> {
        if let Some(block) = &self.current_fs_block {
            Ok(block.fat.iter().fold(0, |a, e| {
                if matches!(e, FATEntry::Free) {
                    a + 1
                } else {
                    a
                }
            }))
        } else {
            Err(Error::NoDevice)
        }
    }

    pub(super) fn dump_current_fs(&self) -> Result<Vec<u8>> {
        if let Some(b) = &self.current_fs_block {
            let block = match b.write() {
                Ok(bl) => bl,
                Err(e) => {
                    eprintln!("{e}");
                    return Err(Error::Io);
                }
            };
            Ok(block)
        } else {
            Err(Error::NoDevice)
        }
    }

    fn increment_seqno(&mut self) {
        if let Some(block) = &mut self.current_fs_block {
            block.footer.seqno = block.footer.seqno.wrapping_add(1);
        }
    }

    fn update_fs(&mut self) -> Result<()> {
        let next_index = (self.current_fs_index.wrapping_sub(1) % 16) + 0xFF0;

        self.increment_seqno();

        if let Some(b) = &self.current_fs_block {
            let block = match b.write() {
                Ok(bl) => bl,
                Err(e) => {
                    eprintln!("{e}");
                    return Err(Error::Io);
                }
            };
            self.write_block_spare(&block, &self.current_fs_spare, next_index)?;

            self.init_fs()
        } else {
            Err(Error::Io)
        }
    }

    fn check_seqno(&mut self, block_num: u32, current_seqno: u32) -> Result<u32> {
        let (block, spare) = self.read_block_spare(block_num)?;
        let seqno = num_from_arr(&block[0x3FF8..0x3FFC]);
        if seqno > current_seqno {
            self.current_fs_block = match FSBlock::read(&block) {
                Ok(b) => Some(b),
                Err(e) => {
                    eprintln!("{e}");
                    return Err(Error::Io);
                }
            };
            self.current_fs_spare = spare;
            self.current_fs_index = block_num - 0xFF0;
            Ok(seqno)
        } else {
            Ok(current_seqno)
        }
    }

    pub(super) fn get_current_fs(&mut self) -> Result<bool> {
        let mut current_seqno: u32 = 0;
        for i in (0xFF0..=0xFFF).rev() {
            current_seqno = self.check_seqno(i, current_seqno)?;
        }
        Ok(current_seqno != 0)
    }

    pub(super) fn list_file_blocks(&self, filename: &str) -> Result<Option<Vec<u16>>> {
        if let Some(block) = &self.current_fs_block {
            let file = match self.find_file(filename)? {
                Some(f) => f,
                None => return Ok(None),
            };
            let mut rv = vec![];
            let mut next_block = file.start;
            while let FATEntry::Chain(b) = next_block {
                rv.push(b);
                next_block = block.fat[b as usize];
            }
            Ok(Some(rv))
        } else {
            Err(Error::NoDevice)
        }
    }

    pub(super) fn list_files(&self) -> Result<Vec<(String, u32)>> {
        if let Some(block) = &self.current_fs_block {
            Ok(block
                .entries
                .iter()
                .filter_map(|e| {
                    if e.valid() {
                        Some((e.get_fullname(), e.size))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>())
        } else {
            Err(Error::NoDevice)
        }
    }

    fn free_blocks(&mut self, mut next_block: FATEntry) {
        if let Some(block) = &mut self.current_fs_block {
            while let FATEntry::Chain(b) = next_block {
                let b = b as usize;
                next_block = block.fat[b];
                block.fat[b] = FATEntry::Free;
            }
        }
    }

    pub(crate) fn delete_file(&mut self, filename: &str) -> Result<()> {
        let file = match self.get_file(filename)? {
            Some(f) => f,
            None => return Ok(()),
        };
        let start = file.start;
        file.clear();

        self.free_blocks(start);
        Ok(())
    }

    pub(super) fn delete_file_and_update(&mut self, filename: &str) -> Result<()> {
        self.delete_file(filename)?;
        self.update_fs()
    }

    pub(super) fn get_stats(&self) -> Result<(usize, usize, usize, u32)> {
        if let Some(block) = &self.current_fs_block {
            let (free, used, bad) = block.fat.iter().fold((0, 0, 0), |(a, b, c), e| match e {
                FATEntry::Free => (a + 1, b, c),
                FATEntry::BadBlock => (a, b, c + 1),
                _ => (a, b + 1, c),
            });
            Ok((free, used, bad, block.footer.seqno))
        } else {
            Err(Error::NoDevice)
        }
    }

    fn read_blocks(&self, file: &FileEntry) -> Result<Option<Vec<u8>>> {
        if let Some(block) = &self.current_fs_block {
            let mut filebuf = Vec::with_capacity(file.size as usize);
            let mut next_block = file.start;
            let bar = ProgressBar::new(file.size.into()).with_style(
                ProgressStyle::with_template(
                    "{wide_bar} {bytes}/{total_bytes}, eta {eta} ({binary_bytes_per_sec})",
                )
                .unwrap(),
            );
            while filebuf.len() < file.size as usize && let FATEntry::Chain(b) = next_block {
                let (read_block, _) = self.read_block_spare(b.into())?;
                let to_write = &read_block[..read_block.len().min(file.size as usize - filebuf.len())];
                bar.inc(to_write.len() as u64);
                filebuf.extend(to_write);
                next_block = block.fat[b as usize];
            }
            Ok(Some(filebuf))
        } else {
            Err(Error::NoDevice)
        }
    }

    pub(super) fn read_file(&self, filename: &str) -> Result<Option<Vec<u8>>> {
        let file = match self.find_file(filename)? {
            Some(f) => f,
            None => return Ok(None),
        };
        self.read_blocks(file)
    }

    fn calculate_file_checksum(data: &[u8]) -> u32 {
        data.iter().fold(0u32, |a, &e| a.wrapping_add(e as u32))
    }

    fn validate_file_write(
        &mut self,
        filename: &str,
        chksum: u32,
        required_blocks: usize,
    ) -> Result<bool> {
        match self.find_file(filename)? {
            Some(_) => {
                if self.file_checksum_cmp(
                    filename,
                    chksum,
                    (required_blocks * BLOCK_SIZE) as u32,
                )? {
                    Ok(false)
                } else {
                    let block_count = self.get_file_block_count(filename)?;
                    self.delete_file(filename)?;
                    Ok(required_blocks < self.get_free_block_count()? + block_count)
                }
            }
            None => Ok(required_blocks <= self.get_free_block_count()?),
        }
    }

    fn write_file_blocks(
        &self,
        data: &[u8],
        blocks_to_write: &[u16],
        required_blocks: usize,
    ) -> Result<()> {
        const BLANK_SPARE: [u8; SPARE_SIZE] = [0xFF; SPARE_SIZE];

        for (block, &index) in data.chunks(BLOCK_SIZE).zip(blocks_to_write) {
            let mut block = block.to_vec();
            block.extend(vec![0x00; BLOCK_SIZE - block.len()]);
            self.write_block_spare(&block, &BLANK_SPARE, index.into())?;
        }

        Ok(())
    }

    fn find_blank_file_entry(&mut self) -> Result<&mut FileEntry> {
        if let Some(block) = &mut self.current_fs_block {
            for entry in &mut block.entries {
                if !entry.valid() {
                    return Ok(entry);
                }
            }
            Err(Error::NoMem)
        } else {
            Err(Error::NoDevice)
        }
    }

    fn write_file_entry(
        &mut self,
        filename: &str,
        start_block: usize,
        filesize: u32,
    ) -> Result<()> {
        let entry = self.find_blank_file_entry()?;
        entry.set_filename(filename)?;
        entry.valid = FileValid::Valid;
        entry.start = FATEntry::Chain(start_block as u16);
        entry.size = filesize;

        Ok(())
    }

    fn find_next_free_block(&self, start_at: usize) -> Result<usize> {
        if let Some(block) = &self.current_fs_block {
            for (index, i) in block.fat.iter().enumerate() {
                if matches!(i, FATEntry::Free) {
                    return Ok(index);
                }
            }
            Err(Error::NoMem)
        } else {
            Err(Error::NoDevice)
        }
    }

    fn update_fs_links(&mut self, start_block: usize, required_blocks: usize) -> Result<Vec<u16>> {
        let mut free_blocks = Vec::with_capacity(required_blocks);
        free_blocks.push(start_block as u16);
        let mut prev = required_blocks as u16;
        for _ in 0..required_blocks - 1 {
            let next = self.find_next_free_block(prev as usize + 1)? as u16;
            free_blocks.push(next);
            prev = next;
        }
        if let Some(block) = &mut self.current_fs_block {
            let mut current_block = free_blocks[0];
            for &next_block in &free_blocks[1..free_blocks.len()] {
                block.fat[current_block as usize] = FATEntry::Chain(next_block);

                current_block = next_block;
            }
            block.fat[current_block as usize] = FATEntry::EndOfChain;

            Ok(free_blocks)
        } else {
            Err(Error::NoDevice)
        }
    }

    fn write_blocks_to_temp_file(&mut self, data: &[u8], required_blocks: usize) -> Result<()> {
        let start_block = self.find_next_free_block(0x40)?;
        self.write_file_entry(
            "temp.tmp",
            start_block,
            (required_blocks * BLOCK_SIZE) as u32,
        )?;

        let blocks_to_write = self.update_fs_links(start_block, required_blocks)?;
        self.write_file_blocks(data, &blocks_to_write, required_blocks)
    }

    fn check_and_cleanup_temp_file(
        &mut self,
        filename: &str,
        chksum: u32,
        required_blocks: usize,
    ) -> Result<()> {
        if self.file_checksum_cmp("temp.tmp", chksum, (required_blocks * BLOCK_SIZE) as u32)? {
            self.rename_file("temp.tmp", filename)
        } else {
            Err(Error::Io)
        }
    }

    pub(super) fn write_file(&mut self, data: &[u8], filename: &str) -> Result<()> {
        let chksum = Self::calculate_file_checksum(data);
        let required_blocks = Self::bytes_to_blocks(data.len());

        self.validate_file_write(filename, chksum, required_blocks)?;
        self.write_blocks_to_temp_file(data, required_blocks)?;
        self.update_fs()?;

        self.check_and_cleanup_temp_file(filename, chksum, required_blocks)?;

        self.update_fs()?;

        Ok(())
    }
}
