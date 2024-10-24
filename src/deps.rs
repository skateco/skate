use std::ops::Deref;
use crate::exec::{RealExec, ShellExec};
use crate::filestore::{FileStore, Store};

pub trait With<T: ?Sized> {
    fn get(& self) -> Box<T>;
}

pub trait WithRef<'a, T: ?Sized> {
    fn get_ref(&'a self) -> &'a Box<T>;
}


pub struct Deps {
    pub store: Box<dyn Store>
}

impl With<dyn Store> for Deps {
    fn get(&self) -> Box<dyn Store> {
        Box::new(FileStore::new())
    }
}

// impl<'a> WithRef<'a, dyn Store> for Deps {
//     fn get_ref(&'a self) -> &'a Box<dyn Store> {
//         &self.store
//     }
// }

impl With<dyn ShellExec> for Deps {
    fn get(&self) -> Box<dyn ShellExec> {
        Box::new(RealExec{})
    }
}

