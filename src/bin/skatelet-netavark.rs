#![cfg(target_os = "linux")]

use std::error::Error;
use skate::netavark;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    netavark();
    Ok(())
}
