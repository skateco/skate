#![warn(unused_extern_crates)]
use std::error::Error;
use skate::skate;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    skate().await
}
