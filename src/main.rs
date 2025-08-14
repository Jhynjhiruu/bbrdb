use std::fs::{read, write};

use anyhow::Result;
use bbrdb::{scan_devices, Handle};

fn main() -> Result<()> {
    let devices = scan_devices()?;

    println!("{devices:?}");

    let device = &devices[0];

    println!("{device:?}");

    let mut handle = Handle::new(device)?;

    println!("id: {:08X}", handle.GetBBID()?);

    handle.Init()?;

    println!("initialised: {}", handle.initialised()?);

    //let bad = handle.ScanBadBlocks()?;
    //println!("{bad:?}");

    //let nand = handle.DumpNAND()?;

    //write("nand.bin", nand)?;

    println!("files: {:#?}", handle.ListFiles()?);

    let temp = handle.ReadFile("temp.tmp")?;
    if let Some(t) = temp {
        write("temp.tmp", t)?;
    }

    /*while handle.ListFiles()?.contains(&"test.bin".to_string()) {
        handle.DeleteFile("test.bin")?;
    }*/

    let sigs = handle.ReadFile("sig.db")?;
    if let Some(s) = sigs {
        write("sig.db", s)?;
    }

    let config = handle.ReadFile("config.ini")?;
    if let Some(c) = config {
        write("config.ini", c)?;
    }

    let data = read("test.bin")?;
    handle.WriteFile(&data, "test.bin")?;

    let test = handle.ReadFile("test.bin")?;
    if let Some(t) = test {
        write("test.bin.new", t)?;
    }

    Ok(())
}
