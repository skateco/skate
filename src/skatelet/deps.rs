use crate::deps::{With, WithDB};
use crate::exec::{RealExec, ShellExec};
use sqlx::SqlitePool;

pub struct SkateletDeps {
    pub db: SqlitePool,
}

impl WithDB for SkateletDeps {
    fn get_db(&self) -> SqlitePool {
        self.db.clone()
    }
}

impl With<dyn ShellExec> for SkateletDeps {
    fn get(&self) -> Box<dyn ShellExec> {
        Box::new(RealExec {})
    }
}
