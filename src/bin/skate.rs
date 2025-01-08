#![warn(unused_extern_crates)]

use skate::deps::Deps;
use skate::{skate};

#[tokio::main]
async fn main() {

    let deps = Deps {};
    let res = skate(deps).await;

    match res {
        Ok(_) => (),
        Err(e) => {
            eprintln!("{}", e.to_string());
            std::process::exit(1);
        }
    }
}
