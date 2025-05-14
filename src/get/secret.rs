use crate::filestore::ObjectListItem;
use crate::get::lister::NameFilters;
use crate::get::ResourceLister;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::util::age;
use k8s_openapi::api::core::v1::Secret;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct SecretLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct SecretListItem {
    pub namespace: String,
    pub name: String,
    pub data: usize,
    pub age: String,
}

impl From<ObjectListItem> for SecretListItem {
    fn from(item: ObjectListItem) -> Self {
        let data: usize = match item.manifest {
            Some(ref m) => {
                let secret = serde_yaml::from_value::<Secret>(m.clone()).unwrap_or_default();
                match secret.string_data {
                    Some(data) => data.len(),
                    None => match secret.data {
                        Some(data) => data.len(),
                        None => 0,
                    },
                }
            }
            None => 0,
        };

        SecretListItem {
            namespace: item.name.namespace.clone(),
            name: item.name.name.clone(),
            data,
            age: age(item.created_at),
        }
    }
}
impl NameFilters for SecretListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.to_string()
    }
}

impl ResourceLister<SecretListItem> for SecretLister {}
