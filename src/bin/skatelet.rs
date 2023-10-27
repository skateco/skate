use std::error::Error;
use tokio;
use skate::skatelet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    skatelet().await
}
