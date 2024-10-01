#![warn(unused_extern_crates)]
use std::error::Error;
use skate::skatelet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    skatelet().await
}
