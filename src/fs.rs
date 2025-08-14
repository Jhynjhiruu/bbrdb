use std::ffi::CString;
use std::io::Cursor;
use std::iter::repeat;
use std::num::Wrapping;

use binrw::binrw;
use binrw::BinRead;
use binrw::BinResult;
use binrw::BinWrite;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rusb::UsbContext;

use crate::commands::Command;
use crate::constants::BLOCK_SIZE;
use crate::constants::NUM_FATS;
use crate::constants::SPARE_SIZE;
use crate::error::*;
use crate::rdb::RDBCommand;
use crate::require_fat;
use crate::require_init;
use crate::Handle;

fn next_block_size(size: u32) -> u32 {
    (size + (BLOCK_SIZE - 1) as u32) & !((BLOCK_SIZE - 1) as u32)
}

#[derive(Debug)]
pub struct Fat {
    entries: Vec<FATEntry>,
    files: Vec<FileEntry>,
    seqno: u32,
    blkno: u32,
}

#[derive(Debug)]
struct _Fat {
    entries: Vec<FATEntry>,
    files: Vec<FileEntry>,
    seqno: Option<u32>,
    blkno: Option<u32>,
}

impl From<_Fat> for Fat {
    fn from(value: _Fat) -> Self {
        Self {
            entries: value.entries,
            files: value.files,
            seqno: value.seqno.unwrap(),
            blkno: value.blkno.unwrap(),
        }
    }
}

impl Fat {
    pub fn check(&self) -> Result<()> {
        for file in &self.files {
            if file.valid != FileValid::Valid {
                continue;
            }

            let mut b = &file.start;
            while b != &FATEntry::EndOfChain {
                match b {
                    FATEntry::Chain(n) => b = &self.entries[*n as usize],
                    FATEntry::Free | FATEntry::BadBlock | FATEntry::Reserved => {
                        println!("Found invalid file");
                    }
                    FATEntry::EndOfChain => unreachable!(),
                }
            }
        }

        Ok(())
    }

    pub fn blocks(&self) -> Vec<FSBlock> {
        let blocks = self.entries.chunks(0x1000);

        blocks
            .into_iter()
            .enumerate()
            .map(|(index, b)| FSBlock {
                fat: b.try_into().unwrap(),
                entries: if index == 0 {
                    self.files
                        .iter()
                        .cloned()
                        .chain(repeat(FileEntry::default()))
                        .take(409)
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap()
                } else {
                    repeat(FileEntry::default())
                        .take(409)
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap()
                },
                footer: FSFooter {
                    fs_type: if index == 0 {
                        FSType::Bbfs
                    } else {
                        FSType::Bbfl
                    },
                    seqno: self.seqno.wrapping_add(1),
                    link_block: 0,
                    chksum: 0,
                },
            })
            .collect()
    }
}

impl _Fat {
    pub fn new() -> Self {
        Self {
            entries: vec![],
            files: vec![],
            seqno: None,
            blkno: None,
        }
    }

    pub fn add_block(&mut self, block: FSBlock, num: u32) -> u16 {
        let link = block.footer.link_block;

        self.entries.extend(block.fat);
        if self.files.is_empty() {
            self.files.extend(block.entries);
        }
        match self.seqno {
            Some(n) => assert_eq!(n, block.footer.seqno),
            None => self.seqno = Some(block.footer.seqno),
        }
        match self.blkno {
            Some(n) => assert_eq!(n, num),
            None => self.blkno = Some(num),
        }

        link
    }
}

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
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FileValid {
    #[brw(magic = 0x00u8)]
    Invalid,
    #[brw(magic = 0x01u8)]
    Valid,
}

#[binrw]
#[derive(Debug, Clone)]
pub struct FileEntry {
    name: [u8; 8],
    ext: [u8; 3],
    valid: FileValid,
    start: FATEntry,
    pad: u16, // used by libdragon for non-block file sizes
    size: u32,
}

impl Default for FileEntry {
    fn default() -> Self {
        Self {
            name: Default::default(),
            ext: Default::default(),
            valid: FileValid::Invalid,
            start: FATEntry::Free,
            pad: 0,
            size: 0,
        }
    }
}

impl FileEntry {
    pub(crate) fn format_name(&self) -> String {
        let name = self.name.split(|b| b == &0).next().unwrap();
        let name = String::from_utf8_lossy(name);

        let ext = self.ext.split(|b| b == &0).next().unwrap();
        let ext = String::from_utf8_lossy(ext);

        if ext == "" {
            name.into_owned()
        } else {
            format!("{}.{}", name, ext)
        }
    }

    pub(crate) fn valid(&self) -> bool {
        self.valid == FileValid::Valid
    }

    pub(crate) fn set_name(&mut self, filename: &str) -> Result<()> {
        let filename = filename.to_lowercase();
        let split = filename.split('.').collect::<Vec<_>>();
        let (name, ext) = if split.len() > 1 {
            (split[0], split[1])
        } else {
            (split[0], "")
        };

        if name.len() > 8 || ext.len() > 3 {
            return Err(LibBBRDBError::FileNameTooLong(filename));
        }

        self.name
            .copy_from_slice((name.to_owned() + &"\0".repeat(8 - name.len())).as_bytes());
        self.ext
            .copy_from_slice((ext.to_owned() + &"\0".repeat(3 - ext.len())).as_bytes());

        Ok(())
    }

    pub(crate) fn set_size(&mut self, filesize: u32) {
        let padded = next_block_size(filesize);
        let diff = padded - filesize;

        self.size = padded;
        self.pad = diff as u16;
    }

    pub(crate) fn clear(&mut self) {
        *self = Default::default();
    }

    pub(crate) fn size(&self) -> usize {
        self.size as usize - self.pad as usize
    }
}

#[binrw]
#[derive(Debug, PartialEq, Eq)]
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

const FAT_CHECKSUM: u16 = 0xCAD7;

fn check_fat_checksum(data: &[u8]) -> Result<()> {
    let sum: Wrapping<u16> = data
        .chunks(2)
        .map(|c| Wrapping(u16::from_be_bytes(c.try_into().unwrap())))
        .sum();
    if sum.0 != FAT_CHECKSUM {
        Err(LibBBRDBError::InvalidFATChecksum(sum.0))
    } else {
        Ok(())
    }
}

fn fix_fat_checksum(data: &mut [u8]) {
    let sum: Wrapping<u16> = data[..0x3FFE]
        .as_ref()
        .chunks(2)
        .map(|c| Wrapping(u16::from_be_bytes(c.try_into().unwrap())))
        .sum();
    let checksum = FAT_CHECKSUM.wrapping_sub(sum.0);
    data[0x3FFE..].copy_from_slice(&checksum.to_be_bytes());
}

pub struct CardStats {
    pub free: usize,
    pub used: usize,
    pub bad: usize,
    pub seqno: u32,
}

impl<C: UsbContext> Handle<C> {
    fn write_fat_block(&mut self, block: u32, fs: FSBlock) -> Result<()> {
        let mut data = vec![];
        let mut cursor = Cursor::new(&mut data);
        fs.write_be(&mut cursor)?;

        fix_fat_checksum(&mut data);

        self.write_blocks(block, &[&data])
    }

    fn read_fat_block(&self, block: u32) -> Result<FSBlock> {
        let (nand, _) = self.read_blocks_spare(block, 1)?;

        check_fat_checksum(&nand)?;

        let mut cursor = Cursor::new(&nand);
        Ok(FSBlock::read_be(&mut cursor)?)
    }

    fn find_best_fat(&self, cardsize: u32) -> Result<Fat> {
        let mut fat = _Fat::new();

        if cardsize == 0 {
            return Err(LibBBRDBError::UnhandledCardSize);
        }

        let mut best_seqno = 0;
        let mut best_fat = None;

        for f in 0..NUM_FATS {
            let fat = self.read_fat_block(cardsize - f - 1);
            if let Ok(b) = fat {
                if b.footer.fs_type == FSType::Bbfs && b.footer.seqno >= best_seqno {
                    best_seqno = b.footer.seqno;
                    best_fat = Some(f);
                }
            }
        }

        if let Some(f) = best_fat {
            let mut link = cardsize - f - 1;

            while link != 0 {
                let b = self.read_fat_block(link)?;

                link = fat.add_block(b, f) as u32;
            }

            Ok(fat.into())
        } else {
            Err(LibBBRDBError::NoFAT)
        }
    }

    pub(crate) fn read_fat(&self, cardsize: u32) -> Result<Fat> {
        let fat = self.find_best_fat(cardsize)?;

        fat.check()?;

        Ok(fat)
    }

    fn get_file(&mut self, filename: &str) -> Result<Option<&mut FileEntry>> {
        let filename = filename.to_lowercase();
        require_fat!(mut self, _p, fat {
            for file in &mut fat.files {
                if file.valid() && file.format_name() == filename {
                    return Ok(Some(file));
                }
            }
            Ok(None)
        })
    }

    fn find_file(&self, filename: &str) -> Result<Option<&FileEntry>> {
        let filename = filename.to_lowercase();
        require_fat!(self, _p, fat {
            for file in &fat.files {
                if file.valid() && file.format_name() == filename {
                    return Ok(Some(file));
                }
            }
            Ok(None)
        })
    }

    fn rename_file(&mut self, from: &str, to: &str) -> Result<()> {
        let from = from.to_lowercase();
        let to = to.to_lowercase();
        if from == to {
            Ok(())
        } else {
            self.delete_file(&to)?;
            match self.get_file(&from)? {
                Some(f) => f.set_name(&to),
                None => Err(LibBBRDBError::FileNotFound(from)),
            }
        }
    }

    fn bytes_to_blocks(bytes: usize) -> usize {
        (bytes + BLOCK_SIZE - 1) / BLOCK_SIZE
    }

    fn get_file_block_count(&self, filename: &str) -> Result<usize> {
        let filename = filename.to_lowercase();
        match self.find_file(&filename)? {
            Some(f) => Ok(Self::bytes_to_blocks(f.size as usize)),
            None => Err(LibBBRDBError::FileNotFound(filename)),
        }
    }

    fn get_free_block_count(&self) -> Result<usize> {
        require_fat!(self, _p, fat {
            Ok(fat.entries.iter().fold(0, |a, e| {
                if matches!(e, FATEntry::Free) {
                    a + 1
                } else {
                    a
                }
            }))
        })
    }

    fn init_fs(&self) -> Result<()> {
        let status = self.command_response(Command::InitFS, 0, 1)?[0];
        if status != 0 {
            Err(CardError::from_i32(status).into())
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "writing")]
    fn update_fs(&mut self) -> Result<()> {
        require_fat!(self, player, fat {
            let mut next_index = fat.blkno;
            let mut next_block = || {
                next_index = next_index.wrapping_add(1) % 16;
                player.cardsize - next_index - 1
            };

            let mut blocks = fat.blocks();

            let mut addrs = vec![];
            for _ in 0..blocks.len() {
                addrs.push(next_block());
            }

            for (index, block) in blocks.iter_mut().enumerate() {
                block.footer.link_block = addrs.get(index + 1).copied().unwrap_or(0) as _;
            }

            for (block, &addr) in blocks.into_iter().zip(&addrs) {
                self.write_fat_block(addr, block)?;
            }

            self.init_fs()
        })
    }

    fn free_blocks(&mut self, mut next_block: FATEntry) -> Result<()> {
        require_fat!(mut self, _p, fat {
            while let FATEntry::Chain(b) = next_block {
                let b = b as usize;
                next_block = fat.entries[b];
                fat.entries[b] = FATEntry::Free;
            }

            Ok(())
        })
    }

    fn delete_file(&mut self, filename: &str) -> Result<()> {
        let filename = filename.to_lowercase();
        let file = match self.get_file(&filename)? {
            Some(f) => f,
            None => return Ok(()),
        };

        let start = file.start;
        file.clear();

        self.free_blocks(start)
    }

    fn read_file_blocks(&self, file: &FileEntry) -> Result<Option<Vec<u8>>> {
        require_fat!(self, _p, fat {
            let mut filebuf = Vec::with_capacity(file.size());
            let mut next_block = file.start;
            let bar = ProgressBar::new(file.size() as u64).with_style(
                ProgressStyle::with_template(
                    "{wide_bar} {bytes}/{total_bytes}, eta {eta} ({binary_bytes_per_sec})",
                )
                .unwrap(),
            );

            while filebuf.len() < file.size() && matches!(next_block, FATEntry::Chain(_)) {
                let FATEntry::Chain(b) = next_block else {
                    unreachable!()
                };

                let (read_block, _) = self.read_blocks_spare(b.into(), 1)?;
                let to_write =
                    &read_block[..read_block.len().min(file.size() - filebuf.len())];
                bar.inc(to_write.len() as u64);
                filebuf.extend(to_write);
                next_block = fat.entries[b as usize];
            }

            Ok(Some(filebuf))
        })
    }

    fn calc_file_checksum(data: &[u8]) -> u32 {
        data.iter().fold(0, |a, &e| a.wrapping_add(e as _))
    }

    fn checksum_file(&self, filename: &str, chksum: u32, size: u32) -> Result<bool> {
        let filename = filename.to_lowercase();
        FileEntry::default().set_name(&filename)?;

        let name = CString::new(filename.as_bytes())
            .map_err(|_| LibBBRDBError::InvalidFilename(filename))?;
        let name = name.as_bytes_with_nul();

        let len = name.len() as u32;

        let mut name = name.to_vec();
        while name.len() % 4 != 0 {
            name.push(0);
        }
        //let padded_len = (len + 3) & !3;

        self.send_command(Command::ChksumFile, len)?;
        self.write_data(RDBCommand::HostData, name)?;

        let checksum_data = {
            let mut v = vec![];
            v.extend(chksum.to_be_bytes());
            v.extend(size.to_be_bytes());
            v
        };
        self.write_data(RDBCommand::HostData, checksum_data)?;

        let status = self.check_cmd_response(Command::ChksumFile, 1)?[0];
        Ok(status == 0)
    }

    fn validate_file_write(&mut self, filename: &str, chksum: u32, size: u32) -> Result<bool> {
        let filename = filename.to_lowercase();
        match self.find_file(&filename)? {
            Some(f) => {
                if self.checksum_file(&filename, chksum, f.size() as u32)? {
                    Ok(false)
                } else {
                    let block_count = self.get_file_block_count(&filename)?;
                    if size < ((self.get_free_block_count()? + block_count) * BLOCK_SIZE) as u32 {
                        Ok(true)
                    } else {
                        Err(LibBBRDBError::FileTooBig(
                            filename.to_string(),
                            size,
                            ((self.get_free_block_count()? + block_count) * BLOCK_SIZE) as u32,
                        ))
                    }
                }
            }
            None => {
                if size <= (self.get_free_block_count()? * BLOCK_SIZE) as u32 {
                    Ok(true)
                } else {
                    Err(LibBBRDBError::FileTooBig(
                        filename.to_string(),
                        size,
                        (self.get_free_block_count()? * BLOCK_SIZE) as u32,
                    ))
                }
            }
        }
    }

    #[cfg(feature = "writing")]
    fn write_file_blocks(&mut self, data: &[u8], blocks_to_write: &[u16]) -> Result<()> {
        const BLANK_SPARE: [u8; SPARE_SIZE] = [0xFF; SPARE_SIZE];

        require_init!(self, player
        {
            assert!(
                blocks_to_write
                    .iter()
                    .all(|&e| (0x40..(player.cardsize - NUM_FATS)).contains(&(e as u32))),
                "Trying to write to reserved area"
            );

            let chunks = data.chunks(BLOCK_SIZE);

            if blocks_to_write.len() != chunks.len() {
                return Err(LibBBRDBError::IncorrectNumBlocks(
                    chunks.len(),
                    blocks_to_write.len(),
                ));
            }

            let bar = ProgressBar::new(data.len() as u64).with_style(
                ProgressStyle::with_template(
                    "{wide_bar} {bytes}/{total_bytes}, eta {eta} ({binary_bytes_per_sec})",
                )
                .unwrap(),
            );

            for (block, &index) in chunks.zip(blocks_to_write) {
                let mut block = block.to_vec();
                block.extend(vec![0x00; BLOCK_SIZE - block.len()]);
                self.write_blocks_spare(index.into(), &[(&block, &BLANK_SPARE)])?;
                bar.inc(block.len() as u64);
            }

            Ok(())
        })
    }

    fn find_blank_file_entry(&mut self) -> Result<&mut FileEntry> {
        require_fat!(mut self, _p, fat {
            for entry in &mut fat.files {
                if !entry.valid() {
                    return Ok(entry);
                }
            }
            Err(LibBBRDBError::NoEmptyFileSlots)
        })
    }

    #[cfg(feature = "writing")]
    fn write_file_entry(
        &mut self,
        filename: &str,
        start_block: usize,
        filesize: u32,
    ) -> Result<&FileEntry> {
        let filename = filename.to_lowercase();
        let entry = self.find_blank_file_entry()?;
        entry.set_name(&filename)?;
        entry.valid = FileValid::Valid;
        entry.start = FATEntry::Chain(start_block as u16);
        entry.set_size(filesize);

        Ok(entry)
    }

    fn find_next_free_block(&self, start_at: usize) -> Result<usize> {
        require_fat!(self, _p, fat {
            for (index, i) in fat.entries[start_at..].iter().enumerate() {
                if matches!(i, FATEntry::Free) {
                    return Ok(index + start_at);
                }
            }
            Err(LibBBRDBError::NoFreeBlocks)
        })
    }

    #[cfg(feature = "writing")]
    fn update_fs_links(&mut self, start_block: usize, size: u32) -> Result<Vec<u16>> {
        require_fat!(self, _p, _f { Ok(()) })?;

        let mut free_blocks = Vec::with_capacity(next_block_size(size) as usize);
        let mut prev = start_block as u16;
        free_blocks.push(prev);

        let mut allocated_size = BLOCK_SIZE as u32;
        while allocated_size < size {
            let next = self.find_next_free_block(prev as usize + 1)? as u16;
            free_blocks.push(next);
            prev = next;
            allocated_size += BLOCK_SIZE as u32;
        }

        require_fat!(mut self, _p, fat {
            let mut current_block = free_blocks[0];
            for &next_block in &free_blocks[1..] {
                fat.entries[current_block as usize] = FATEntry::Chain(next_block);
                current_block = next_block;
            }
            fat.entries[current_block as usize] = FATEntry::EndOfChain;

            Ok(free_blocks)
        })
    }

    #[cfg(feature = "writing")]
    fn write_blocks_to_temp_file(&mut self, data: &[u8]) -> Result<()> {
        self.delete_file("temp.tmp")?;

        let start_block = self.find_next_free_block(0x40)?;
        let size = data.len() as u32;

        let entry = self.write_file_entry("temp.tmp", start_block, size)?;
        let written_size = entry.size() as u32;

        let blocks_to_write = self.update_fs_links(start_block, written_size)?;
        self.write_file_blocks(data, &blocks_to_write)
    }

    #[cfg(not(feature = "raw_access"))]
    fn check_and_cleanup_temp_file(
        &mut self,
        filename: &str,
        chksum: u32,
        size: u32,
    ) -> Result<()> {
        let filename = filename.to_lowercase();
        if self.checksum_file("temp.tmp", chksum, size)? {
            self.rename_file("temp.tmp", &filename)
        } else {
            Err(LibBBRDBError::ChecksumFailed(filename, chksum))
        }
    }

    #[cfg(feature = "writing")]
    #[allow(non_snake_case)]
    pub fn DeleteFile(&mut self, filename: &str) -> Result<()> {
        let filename = filename.to_lowercase();
        self.delete_file(&filename)?;
        self.update_fs()
    }

    #[cfg(feature = "writing")]
    #[allow(non_snake_case)]
    pub fn RenameFile(&mut self, from: &str, to: &str) -> Result<()> {
        let from = from.to_lowercase();
        let to = to.to_lowercase();
        self.rename_file(&from, &to)?;
        self.update_fs()
    }

    #[allow(non_snake_case)]
    pub fn CardStats(&self) -> Result<CardStats> {
        require_fat!(self, _p, fat {
            let (free, used, bad) = fat.entries.iter().fold((0, 0, 0), |(a, b, c), e| match e {
                FATEntry::Free => (a + 1, b, c),
                FATEntry::BadBlock => (a, b, c + 1),
                _ => (a, b + 1, c),
            });

            Ok(CardStats { free, used, bad, seqno: fat.seqno })
        })
    }

    #[allow(non_snake_case)]
    pub fn ReadFile(&self, filename: &str) -> Result<Option<Vec<u8>>> {
        let filename = filename.to_lowercase();
        let file = match self.find_file(&filename)? {
            Some(f) => f,
            None => return Ok(None),
        };
        self.read_file_blocks(file)
    }

    #[allow(non_snake_case)]
    pub fn ListFiles(&self) -> Result<Vec<(String, usize)>> {
        require_fat!(self, _p, fat {
            Ok(fat.files.iter().filter_map(|f| if f.valid() { Some((f.format_name(), f.size())) } else { None }).collect())
        })
    }

    #[allow(non_snake_case)]
    pub fn DumpCurrentFS(&self) -> Result<Vec<u8>> {
        require_fat!(self, _p, fat {
            let mut data = vec![];

            for block in fat.blocks() {
                let mut blk = vec![];
                let mut cursor = Cursor::new(&mut blk);
                block.write_be(&mut cursor)?;

                fix_fat_checksum(&mut blk);

                data.extend(blk);
            }

            Ok(data)
        })
    }

    #[cfg(feature = "writing")]
    #[allow(non_snake_case)]
    pub fn WriteFile(&mut self, data: &[u8], filename: &str) -> Result<()> {
        let filename = filename.to_lowercase();

        let chksum = Self::calc_file_checksum(data);
        let size = data.len() as u32;

        if !self.validate_file_write(&filename, chksum, size)? {
            println!("Checksum matches existing file");
            return Ok(());
        }

        self.delete_file(&filename)?;

        self.write_blocks_to_temp_file(data)?;
        self.update_fs()?;

        self.check_and_cleanup_temp_file(&filename, chksum, size)?;
        self.update_fs()
    }
}
