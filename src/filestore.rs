use crate::errors::SkateError;
use crate::skatelet::database::resource::{Resource, ResourceType};
use crate::skatelet::VAR_PATH;
use crate::spec::cert::ClusterIssuer;
use crate::util::{metadata_name, NamespacedName};
use anyhow::anyhow;
use chrono::{DateTime, Local};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{kind, Metadata};
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::error::Error;
use std::fs::{create_dir_all, DirEntry};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tabled::Tabled;

// all dirs/files live under /var/lib/skate/store
// One directory for
// - ingress
// - cron
// directory structure example is
// /var/lib/skate/store/ingress/ingress-name.namespace/80.conf
// /var/lib/skate/store/ingress/ingress-name.namespace/443.conf
#[derive(Clone)]
pub struct FileStore {
    base_path: String,
}

#[derive(Tabled, Debug, Clone, PartialEq, Deserialize, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct ObjectListItem {
    pub resource_type: ResourceType,
    pub name: NamespacedName,
    pub manifest_hash: String,
    #[tabled(skip)]
    pub manifest: Option<Value>,
    pub updated_at: DateTime<Local>,
    pub created_at: DateTime<Local>,
    pub path: String,
}

impl ObjectListItem {
    pub(crate) fn from_resource_vec(p0: Vec<Resource>) -> Result<Vec<ObjectListItem>> {
        let mut out = vec![];
        for res in p0 {
            res.try_into()
        }
    }
}

impl ObjectListItem {
    fn from_k8s_resource(
        res: &(impl Metadata<Ty = ObjectMeta> + Serialize),
        path: Option<&str>,
    ) -> Self {
        let kind = kind(res);

        let obj = ObjectListItem {
            resource_type: ResourceType::from_str(kind).expect("unexpected resource type"),
            name: metadata_name(res),
            manifest_hash: res
                .metadata()
                .labels
                .as_ref()
                .and_then(|l| l.get("skate.io/hash"))
                .cloned()
                .unwrap_or("".to_string()),
            manifest: Some(
                serde_yaml::to_value(res).expect("failed to serialize kubernetes object"),
            ),
            created_at: Local::now(),
            updated_at: Local::now(),
            path: path.unwrap_or_default().to_string(),
        };
        obj
    }
}

impl TryFrom<&str> for ObjectListItem {
    type Error = Box<dyn Error>;

    fn try_from(dir: &str) -> Result<Self, Self::Error> {
        let dir_path = Path::new(dir);
        let file_name = dir_path
            .file_name()
            .ok_or(anyhow!("failed to get file name"))?;
        let parent = dir_path
            .parent()
            .ok_or(anyhow!("failed to get parent dir"))?
            .file_name()
            .ok_or(anyhow!("failed to get parent dir"))?;
        let parent = parent.to_str().unwrap();

        let ns_name = NamespacedName::from(file_name.to_str().unwrap());

        let hash_file_name = format!("{}/hash", dir);

        let hash = match std::fs::read_to_string(&hash_file_name) {
            Err(_) => {
                warn!("WARNING: failed to read hash file {}", &hash_file_name);
                "".to_string()
            }
            Ok(result) => result,
        };

        let manifest_file_name = format!("{}/manifest.yaml", dir);
        let manifest: Option<Value> = match std::fs::read_to_string(&manifest_file_name) {
            Err(e) => {
                warn!(
                    "WARNING: failed to read manifest file {}: {}",
                    &manifest_file_name, e
                );
                None
            }
            Ok(result) => Some(serde_yaml::from_str(&result).unwrap()),
        };

        let dir_metadata = std::fs::metadata(dir)
            .map_err(|e| anyhow!(e).context(format!("failed to get metadata for {}", dir)))?;
        let created_at = dir_metadata.created()?;
        let updated_at = match std::fs::metadata(&manifest_file_name) {
            Ok(m) => m.modified()?,
            Err(_) => created_at,
        };

        Ok(ObjectListItem {
            resource_type: ResourceType::from_str(parent)?,
            name: ns_name,
            manifest_hash: hash,
            manifest,
            created_at: DateTime::from(created_at),
            updated_at: DateTime::from(updated_at),
            path: dir.to_string(),
        })
    }
}

impl TryFrom<DirEntry> for ObjectListItem {
    type Error = Box<dyn Error>;

    fn try_from(dir_entry: DirEntry) -> Result<Self, Self::Error> {
        let path = dir_entry.path();

        Self::try_from(
            path.to_str()
                .ok_or(anyhow!("failed to convert file name to string"))?,
        )
    }
}

impl From<&Ingress> for ObjectListItem {
    fn from(res: &Ingress) -> Self {
        Self::from_k8s_resource(res, None)
    }
}

impl From<&CronJob> for ObjectListItem {
    fn from(res: &CronJob) -> Self {
        Self::from_k8s_resource(res, None)
    }
}

impl From<&Service> for ObjectListItem {
    fn from(res: &Service) -> Self {
        Self::from_k8s_resource(res, None)
    }
}

impl From<&Secret> for ObjectListItem {
    fn from(res: &Secret) -> Self {
        Self::from_k8s_resource(res, None)
    }
}

impl From<&ClusterIssuer> for ObjectListItem {
    fn from(res: &ClusterIssuer) -> Self {
        Self::from_k8s_resource(res, None)
    }
}

impl TryFrom<Resource> for ObjectListItem {
    type Error = Box<dyn Error>;

    fn try_from(value: Resource) -> Result<Self, Self::Error> {
        let path = format!(
            "{VAR_PATH}/{}/{}/{}",
            &value.resource_type.to_string().to_lowercase(),
            &value.namespace,
            &value.name,
        );

        let manifest =
            serde_json::from_str::<serde_yaml::Value>(&serde_json::to_string(&value.manifest)?)?;

        Ok(ObjectListItem {
            resource_type: value.resource_type,
            name: NamespacedName {
                name: value.name,
                namespace: value.namespace,
            },
            manifest_hash: value.hash,
            manifest: Some(manifest),
            created_at: value.created_at,
            updated_at: value.updated_at,
            path,
        })
    }
}

impl FileStore {
    pub fn new(base_path: String) -> Self {
        FileStore { base_path }
    }

    fn get_path(&self, parts: &[&str]) -> String {
        let mut path = PathBuf::from(self.base_path.clone());
        path.extend(parts);
        path.to_string_lossy().to_string()
    }
}

impl Store for FileStore {
    // will clobber
    fn write_file(
        &self,
        object_type: &str,
        object_name: &str,
        file_name: &str,
        file_contents: &[u8],
    ) -> Result<String, SkateError> {
        let dir = self.get_path(&[object_type, object_name]);
        create_dir_all(&dir)
            .map_err(|e| anyhow!(e).context(format!("failed to create directory {}", dir)))?;
        let file_path = format!(
            "{}/{}/{}/{}",
            self.base_path, object_type, object_name, file_name
        );

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file_path);
        match file.map_err(|e| anyhow!(e).context(format!("failed to create file {}", file_path))) {
            Err(e) => Err(e.into()),
            Ok(mut file) => Ok(file.write_all(file_contents).map(|_| file_path)?),
        }
    }
    fn remove_file(
        &self,
        object_type: &str,
        object_name: &str,
        file_name: &str,
    ) -> Result<(), Box<dyn Error>> {
        let file_path = self.get_path(&[object_type, object_name, file_name]);
        let result = std::fs::remove_file(&file_path)
            .map_err(|e| anyhow!(e).context(format!("failed to remove file {}", file_path)));
        if result.is_err() {
            return Err(result.err().unwrap().into());
        }
        Ok(())
    }
    fn exists_file(&self, object_type: &str, object_name: &str, file_name: &str) -> bool {
        let file_path = self.get_path(&[object_type, object_name, file_name]);
        std::path::Path::new(&file_path).exists()
    }
    // returns true if the object was removed, false if it didn't exist
    fn remove_object(&self, object_type: &str, object_name: &str) -> Result<bool, Box<dyn Error>> {
        let dir = self.get_path(&[object_type, object_name]);
        match std::fs::remove_dir_all(&dir) {
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => Ok(false),
                _ => Err(anyhow!(err)
                    .context(format!("failed to remove directory {}", dir))
                    .into()),
            },
            Ok(_) => Ok(true),
        }
    }
    fn get_object(
        &self,
        object_type: &str,
        object_name: &str,
    ) -> Result<ObjectListItem, Box<dyn Error>> {
        let dir = self.get_path(&[object_type, object_name]);

        let obj = ObjectListItem::try_from(dir.as_str())?;
        Ok(obj)
    }
    fn list_objects(&self, object_type: &str) -> Result<Vec<ObjectListItem>, Box<dyn Error>> {
        let dir = self.get_path(&[object_type]);
        let entries = match std::fs::read_dir(&dir) {
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                _ => {
                    return Err(anyhow!(e)
                        .context(format!("failed to read directory {}", dir))
                        .into())
                }
            },
            Ok(result) => result,
        };

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| anyhow!(e).context("failed to read entry"))?;
            let obj = ObjectListItem::try_from(entry)?;
            result.push(obj);
        }
        Ok(result)
    }
}

pub trait Store {
    // will clobber
    fn write_file(
        &self,
        object_type: &str,
        object_name: &str,
        file_name: &str,
        file_contents: &[u8],
    ) -> Result<String, SkateError>;
    fn remove_file(
        &self,
        object_type: &str,
        object_name: &str,
        file_name: &str,
    ) -> Result<(), Box<dyn Error>>;
    fn exists_file(&self, object_type: &str, object_name: &str, file_name: &str) -> bool;
    // returns true if the object was removed, false if it didn't exist
    fn remove_object(&self, object_type: &str, object_name: &str) -> Result<bool, Box<dyn Error>>;
    fn get_object(
        &self,
        object_type: &str,
        object_name: &str,
    ) -> Result<ObjectListItem, Box<dyn Error>>;
    fn list_objects(&self, object_type: &str) -> Result<Vec<ObjectListItem>, Box<dyn Error>>;
}
