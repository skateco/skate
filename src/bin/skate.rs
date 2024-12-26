#![warn(unused_extern_crates)]

use skate::deps::Deps;
use skate::errors::SkateError;
use skate::{skate};

#[tokio::main]
async fn main() -> Result<(), SkateError> {

    let deps = Deps {};
    skate(deps).await
}
