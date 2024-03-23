use bb::error::{LibBBRDBError, Result};
use bb::BBPlayer;

fn main() -> Result<()> {
    let players = BBPlayer::get_players()?;
    println!("{players:#?}");
    let player = BBPlayer::new(&players[0])?;
    println!("init");
    player.mux()?;

    Ok(())
}
