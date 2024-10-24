use crate::exec::{RealExec, ShellExec};
use crate::filestore::{FileStore, Store};

pub trait With<T: ?Sized> {
    fn construct(&self) -> Box<T>;
}

pub trait WithRef<T: ?Sized> {
    fn construct(&self) -> Box<&T>;
}


pub struct Deps {}

impl With<dyn Store> for Deps {
    fn construct(&self) -> Box<dyn Store> {
        Box::new(FileStore::new())
    }
}

impl With<dyn ShellExec> for Deps {
    fn construct(&self) -> Box<dyn ShellExec> {
        Box::new(RealExec{})
    }
}

