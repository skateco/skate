use crate::deps::{With, WithDB};
use crate::exec::{RealExec, ShellExec};
use crate::filestore::{FileStore, Store};
use crate::skatelet::VAR_PATH;
use sqlx::SqlitePool;

pub struct SkateletDeps {
    pub db: SqlitePool,
}

impl WithDB for SkateletDeps {
    fn get_db(&self) -> SqlitePool {
        self.db.clone()
    }
}

impl With<dyn Store> for SkateletDeps {
    fn get(&self) -> Box<dyn Store> {
        Box::new(FileStore::new(format!("{}/store", VAR_PATH)))
    }
}

// impl<'a> WithRef<'a, dyn Store> for Deps {
//     fn get_ref(&'a self) -> &'a Box<dyn Store> {
//         &self.store
//     }
// }

impl With<dyn ShellExec> for SkateletDeps {
    fn get(&self) -> Box<dyn ShellExec> {
        Box::new(RealExec {})
    }
}
