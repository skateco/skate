#![warn(unused_extern_crates)]

use skate::deps::SkateDeps;
use skate::sind::sind;

#[tokio::main]
async fn main() {
    let deps = SkateDeps {};
    match sind(deps).await {
        Ok(_) => (),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
