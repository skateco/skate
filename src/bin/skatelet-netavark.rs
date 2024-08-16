#![cfg(target_os = "linux")]

use std::error::Error;
use skate::netavark;

fn main() -> Result<(), Box<dyn Error>> {
    netavark();
    Ok(())
}
