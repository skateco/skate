use crate::get::lister::NameFilters;
use crate::get::Lister;
use crate::skatelet::SystemInfo;
use crate::util::age;
use k8s_openapi::api::core::v1::Service;
use tabled::Tabled;

pub(crate) struct ServiceLister {}

#[derive(Tabled)]
#[tabled(rename_all = "UPPERCASE")]
pub struct ServiceListItem {
    pub namespace: String,
    pub name: String,
    pub cluster_ip: String,
    pub external_ip: String,
    pub ports: String,
    pub age: String,
}

impl NameFilters for ServiceListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.to_string()
    }
}

impl Lister<ServiceListItem> for ServiceLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<ServiceListItem> {
        si.services
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .filter(|j| j.filter_names(id, ns))
            .map(|item| {
                let ingress: Service =
                    serde_yaml::from_value(item.manifest.as_ref().unwrap().clone())
                        .unwrap_or_default();
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
            })
            .collect()
    }
}
