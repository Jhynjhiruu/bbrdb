use std::fs::write;

use bb::BBPlayer;
use rusb::Result;

fn main() -> Result<()> {
    let players = BBPlayer::get_players()?;
    println!("{players:#?}");
    let mut player = BBPlayer::new(&players[0])?;
    println!("{player:?}");
    player.Init()?;
    println!("{:04X}", player.GetBBID()?);
    player.SetLED(4)?;
    player.SetTime()?;
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
    player.DumpCurrentFS()?;
    /*let (nand, spare) = player.DumpNAND()?;
    write("nand.bin", nand).unwrap();
    write("spare.bin", spare).unwrap();*/
    let (block, spare) = player.ReadSingleBlock(0)?;
    write("block0.bin", &block).unwrap();
    write("spare0.bin", &spare).unwrap();
    player.WriteSingleBlock(block, spare, 0)?;
    let file = match player.ReadFile("00bbc0de.rec")? {
        Some(b) => b,
        None => {
            eprintln!("File not found");
            vec![]
        }
    };
    write("00bbc0de.rec", file).unwrap();
    Ok(())
}
