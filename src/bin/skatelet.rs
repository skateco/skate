#![warn(unused_extern_crates)]
use skate::errors::SkateError;
use skate::skatelet;

#[tokio::main]
async fn main() -> Result<(), SkateError> {
    skatelet().await
}
