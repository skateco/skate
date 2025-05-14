use crate::filestore::ObjectListItem;
use crate::get::lister::NameFilters;
use crate::get::ResourceLister;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::util::age;
use k8s_openapi::api::networking::v1::Ingress;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct IngressLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct IngressListItem {
    pub namespace: String,
    pub name: String,
    pub class: String,
    pub hosts: String,
    pub address: String,
    pub ports: String,
    pub age: String,
}

impl From<ObjectListItem> for IngressListItem {
    fn from(item: ObjectListItem) -> Self {
        let ingress: Ingress =
            serde_yaml::from_value(item.manifest.as_ref().unwrap().clone()).unwrap_or_default();
        let spec = ingress.spec.unwrap_or_default();

        let hosts = spec
            .rules
            .unwrap_or_default()
            .iter()
            .map(|r| r.host.clone().unwrap_or_default())
            .collect::<Vec<String>>()
            .join(",");
        let age = age(item.created_at);
        let address = "".to_string();
        let class = "external".to_string();
        let ports = "80,443".to_string();
        IngressListItem {
            namespace: item.name.namespace.clone(),
            name: item.name.name.clone(),
            class,
            hosts,
            address,
            ports,
            age,
        }
    }
}

impl NameFilters for IngressListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}

impl ResourceLister<IngressListItem> for IngressLister {}
