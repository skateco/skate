use std::error::Error;
#[cfg(target_os = "linux")]
use skate::netavark;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    netavark();
    Ok(())
}
