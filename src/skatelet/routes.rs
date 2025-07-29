use crate::deps::{With, WithDB};
use crate::errors::SkateError;
use crate::exec::ShellExec;

pub trait RoutesDeps: With<dyn ShellExec> + WithDB {}

pub struct Routes<D: RoutesDeps> {
    pub deps: D,
}

impl<D: RoutesDeps> Routes<D> {
    pub async fn routes(&self) -> Result<(), SkateError> {
        Ok(())
    }
}
