#![warn(unused_extern_crates)]
use skate::skatelet;

#[tokio::main]
async fn main() {
    match skatelet().await {
        Ok(_) => (),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
