#![warn(unused_extern_crates)]

use skate::deps::SkateDeps;
use skate::skate;

#[tokio::main]
async fn main() {
    let deps = SkateDeps {};
    let res = skate(deps).await;

    match res {
        Ok(_) => (),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
