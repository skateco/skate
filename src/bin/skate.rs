#![warn(unused_extern_crates)]
use skate::errors::SkateError;
use skate::skate;

#[tokio::main]
async fn main() -> Result<(), SkateError> {
    skate().await
}
