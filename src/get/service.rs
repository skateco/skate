use crate::filestore::ObjectListItem;
use crate::get::ingress::IngressListItem;
use crate::get::lister::NameFilters;
use crate::get::Lister;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::util::age;
use k8s_openapi::api::core::v1::Service;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct ServiceLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct ServiceListItem {
    pub namespace: String,
    pub name: String,
    pub cluster_ip: String,
    pub external_ip: String,
    pub ports: String,
    pub age: String,
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
