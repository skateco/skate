use std::error::Error;
use std::fs::{create_dir_all, ReadDir};
use std::io::Write;
use anyhow::anyhow;

// all dirs/files live under /var/lib/skate/store
// One directory for
// - ingress
// - cron
// directory structure example is
// /var/lib/skate/store/ingress/ingress-name.namespace/80.conf
// /var/lib/skate/store/ingress/ingress-name.namespace/443.conf
pub struct FileStore {
    base_path: String,
}

impl FileStore {
    pub fn new() -> Self {
        FileStore {
            base_path: "/var/lib/skate/store".to_string()
        }
    }

    // will clobber
    pub fn write_file(&self, object_type: &str, object_name: &str, file_name: &str, file_contents: &[u8]) -> Result<String, Box<dyn Error>> {
        let dir = format!("{}/{}/{}", self.base_path, object_type, object_name);
        create_dir_all(&dir).map_err(|e| anyhow!(e).context(format!("failed to create directory {}", dir)))?;
        let file_path = format!("{}/{}/{}/{}", self.base_path, object_type, object_name, file_name);


        let file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&file_path);
        match file.map_err(|e| anyhow!(e).context(format!("failed to create file {}", file_path))) {
            Err(e) => return Err(e.into()),
            Ok(mut file) => file.write_all(file_contents).map(|_| file_path).map_err(|e| e.into())
        }
    }

    pub fn remove_file(&self, object_type: &str, object_name: &str, file_name: &str) -> Result<(), Box<dyn Error>> {
        let file_path = format!("{}/{}/{}/{}", self.base_path, object_type, object_name, file_name);
        let result = std::fs::remove_file(&file_path).map_err(|e| anyhow!(e).context(format!("failed to remove file {}", file_path)));
        if result.is_err() {
            return Err(result.err().unwrap().into());
        }
        Ok(())
    }

    pub fn exists_file(&self, object_type: &str, object_name: &str, file_name: &str) -> bool {
        let file_path = format!("{}/{}/{}/{}", self.base_path, object_type, object_name, file_name);
        std::path::Path::new(&file_path).exists()
    }

    // returns true if the object was removed, false if it didn't exist
    pub fn remove_object(&self, object_type: &str, object_name: &str) -> Result<bool, Box<dyn Error>> {
        let dir = format!("{}/{}/{}", self.base_path, object_type, object_name);
        match std::fs::remove_dir_all(&dir) {
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => Ok(false),
                _ => Err(anyhow!(err).context(format!("failed to remove directory {}", dir)).into())
            }
            Ok(_) => Ok(true)
        }
    }

    pub fn list_objects(&self, object_type: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let dir = format!("{}/{}", self.base_path, object_type);
        let entries = match std::fs::read_dir(&dir) {
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                _ => return Err(anyhow!(e).context(format!("failed to read directory {}", dir)).into())
            },
            Ok(result) => result
        };
        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| anyhow!(e).context("failed to read entry"))?;
            let path = entry.path();
            let file_name = path.file_name().ok_or(anyhow!("failed to get file name"))?;
            result.push(file_name.to_string_lossy().to_string());
        }
        Ok(result)
    }
}
