

use k8s_openapi::api::networking::v1::Ingress;
use tabled::Tabled;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::age;

pub(crate) struct IngressLister {}

#[derive(Tabled)]
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

impl NameFilters for IngressListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}

impl Lister<IngressListItem> for IngressLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<IngressListItem> {
        si.ingresses.as_ref().unwrap_or(&vec!()).iter().filter(|j| {
            j.filter_names(id, ns)
        }).map(|item| {
            let ingress: Ingress = serde_yaml::from_value(item.manifest.as_ref().unwrap().clone()).unwrap_or_default();
            let spec = ingress.spec.unwrap_or_default();

            let hosts = spec.rules.unwrap_or_default().iter().map(|r| r.host.clone().unwrap_or_default()).collect::<Vec<String>>().join(",");
            let age = age(item.created_at);
            let address = "".to_string();
            let class = "external".to_string();
            let ports = "80,443".to_string();
            IngressListItem{
                namespace: item.name.namespace.clone(),
                name: item.name.name.clone(),
                class,
                hosts,
                address,
                ports,
                age,
            }

        }).collect()
    }
}
