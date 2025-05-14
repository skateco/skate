use crate::get::lister::NameFilters;
use crate::get::Lister;
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

impl NameFilters for SecretListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.to_string()
    }
}

impl Lister<SecretListItem> for SecretLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<SecretListItem> {
        let secrets = si
            .resources
            .iter()
            .filter(|r| r.resource_type == ResourceType::Secret);
        secrets
            .filter(|j| j.filter_names(id, ns))
            .map(|item| {
                let data: usize = match item.manifest {
                    Some(ref m) => {
                        let secret =
                            serde_yaml::from_value::<Secret>(m.clone()).unwrap_or_default();
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
            })
            .collect()
    }
}
