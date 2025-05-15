use crate::filestore::ObjectListItem;
use crate::get::lister::NameFilters;
use crate::util::age;
use k8s_openapi::api::core::v1::Service;
use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct ServiceListItem {
    #[serde(skip)]
    pub namespace: String,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub cluster_ip: String,
    #[serde(skip)]
    pub external_ip: String,
    #[serde(skip)]
    pub ports: String,
    #[serde(skip)]
    pub age: String,
    #[tabled(skip)]
    #[serde(flatten)]
    pub manifest: serde_yaml::Value,
}

impl From<ObjectListItem> for ServiceListItem {
    fn from(item: ObjectListItem) -> Self {
        let ingress: Service =
            serde_yaml::from_value(item.manifest.as_ref().unwrap().clone()).unwrap_or_default();
        let spec = ingress.spec.unwrap_or_default();
        let ports: Vec<_> = spec
            .ports
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.port.to_string())
            .collect();

        // TODO get the cluster ip

        let age = age(item.created_at);
        ServiceListItem {
            namespace: item.name.namespace.clone(),
            name: item.name.name.clone(),
            cluster_ip: "-".to_string(),
            external_ip: "-".to_string(),
            ports: ports.join(","),
            age,
            manifest: item.manifest.unwrap_or(serde_yaml::Value::Null),
        }
    }
}

impl NameFilters for ServiceListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.to_string()
    }
}
