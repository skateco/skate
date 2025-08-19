use crate::skatelet::database::resource::{Resource, ResourceType};
use crate::spec::cert::ClusterIssuer;
use crate::supported_resources::SupportedResources;
use crate::util::{NamespacedName, SkateLabels, get_skate_label_value, metadata_name};
use anyhow::anyhow;
use chrono::{DateTime, Local};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::{Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::{Metadata, kind};
use log::warn;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::error::Error;
use std::fs::DirEntry;
use std::path::Path;
use std::str::FromStr;
use tabled::Tabled;

// all dirs/files live under /var/lib/skate/store
// One directory for
// - ingress
// - cron
// directory structure example is
// /var/lib/skate/store/ingress/ingress-name.namespace/80.conf
// /var/lib/skate/store/ingress/ingress-name.namespace/443.conf

#[derive(Tabled, Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct ObjectListItem {
    pub resource_type: ResourceType,
    pub name: NamespacedName,
    pub manifest_hash: String,
    #[tabled(skip)]
    pub manifest: Option<Value>,
    pub generation: i64,
    pub updated_at: DateTime<Local>,
    pub created_at: DateTime<Local>,
}

impl ObjectListItem {
    fn from_k8s_resource(res: &(impl Metadata<Ty = ObjectMeta> + Serialize)) -> Self {
        let kind = kind(res);

        let obj = ObjectListItem {
            resource_type: ResourceType::from_str(kind).expect("unexpected resource type"),
            name: metadata_name(res),
            manifest_hash: res
                .metadata()
                .labels
                .as_ref()
                .and_then(|l| l.get(&SkateLabels::Hash.to_string()))
                .cloned()
                .unwrap_or("".to_string()),
            manifest: Some(
                serde_yaml::to_value(res).expect("failed to serialize kubernetes object"),
            ),
            generation: res.metadata().generation.unwrap_or_default(),
            created_at: Local::now(),
            updated_at: Local::now(),
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

        let generation = manifest
            .as_ref()
            .and_then(|m| m["metadata"]["generation"].as_i64())
            .unwrap_or_default();

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
            generation,
            created_at: DateTime::from(created_at),
            updated_at: DateTime::from(updated_at),
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

impl TryFrom<&SupportedResources> for ObjectListItem {
    type Error = Box<dyn Error>;
    fn try_from(resource: &SupportedResources) -> Result<ObjectListItem, Self::Error> {
        let meta = match resource {
            SupportedResources::Pod(p) => &p.metadata,
            SupportedResources::Deployment(d) => &d.metadata,
            SupportedResources::DaemonSet(ds) => &ds.metadata,
            SupportedResources::Ingress(i) => &i.metadata,
            SupportedResources::CronJob(c) => &c.metadata,
            SupportedResources::Secret(s) => &s.metadata,
            SupportedResources::Service(s) => &s.metadata,
            SupportedResources::ClusterIssuer(i) => &i.metadata,
            SupportedResources::Namespace(ns) => &ns.metadata,
        };

        let name = get_skate_label_value(&meta.labels, &SkateLabels::Name).ok_or("no name")?;

        let ns =
            get_skate_label_value(&meta.labels, &SkateLabels::Namespace).ok_or("no namespace")?;

        let hash = get_skate_label_value(&meta.labels, &SkateLabels::Hash).ok_or("no hash")?;

        Ok(ObjectListItem {
            resource_type: resource.into(),
            name: NamespacedName {
                name: name.clone(),
                namespace: ns.clone(),
            },
            manifest_hash: hash.clone(),
            manifest: Some(serde_yaml::to_value(resource)?),
            generation: meta.generation.unwrap_or_default(),
            updated_at: Default::default(),
            created_at: Default::default(),
        })
    }
}

impl From<&Ingress> for ObjectListItem {
    fn from(res: &Ingress) -> Self {
        Self::from_k8s_resource(res)
    }
}

impl From<&CronJob> for ObjectListItem {
    fn from(res: &CronJob) -> Self {
        Self::from_k8s_resource(res)
    }
}

impl From<&Service> for ObjectListItem {
    fn from(res: &Service) -> Self {
        Self::from_k8s_resource(res)
    }
}

impl From<&Secret> for ObjectListItem {
    fn from(res: &Secret) -> Self {
        Self::from_k8s_resource(res)
    }
}

impl From<&ClusterIssuer> for ObjectListItem {
    fn from(res: &ClusterIssuer) -> Self {
        Self::from_k8s_resource(res)
    }
}

impl TryFrom<Resource> for ObjectListItem {
    type Error = Box<dyn Error>;

    fn try_from(value: Resource) -> Result<Self, Self::Error> {
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
            generation: value.generation,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}
