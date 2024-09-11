use std::collections::HashMap;
use k8s_openapi::api::core::v1::Secret;
use tabled::Tabled;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::{SystemInfo};

use crate::util::age;

pub(crate) struct SecretLister {}

#[derive(Tabled)]
#[tabled(rename_all = "UPPERCASE")]
pub struct SecretListItem {
    pub namespace: String,
    pub name: String,
    pub data: usize,
    pub age: String,
}

impl Lister<SecretListItem> for SecretLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<SecretListItem> {
        si.secrets.as_ref().unwrap_or(&vec!()).iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);
        }).map(|item| {
            let data: usize = match item.manifest {
                Some(ref m) => {
                    let secret = serde_yaml::from_value::<Secret>(m.clone()).unwrap_or_default();
                    match secret.string_data {
                        Some(data) => data.len(),
                        None => match secret.data {
                            Some(data) => data.len(),
                            None => 0
                        }
                    }
                }
                None => 0
            };

            SecretListItem {
                namespace: item.name.namespace.clone(),
                name: item.name.name.clone(),
                data,
                age: age(item.created_at),
            }
        }).collect()
    }

    // fn print(&self, items: Vec<ObjectListItem>) {
    //     let map = items.iter().fold(HashMap::<String, Vec<ObjectListItem>>::new(), |mut acc, item| {
    //         acc.entry(item.name.to_string()).or_insert(vec![]).push(item.clone());
    //         acc
    //     });
    //
    //     macro_rules! cols {
    //         () => ("{0: <15}  {1: <15}  {2: <15}  {3: <15}  {4: <10}")
    //     }
    //     println!(
    //         cols!(),
    //         "NAMESPACE", "NAME", "TYPE", "DATA", "AGE",
    //     );
    //
    //
    //     for item in map {
    //
    //         let data: usize = match item.1.first().unwrap().manifest{
    //             Some(ref m) => {
    //                 let secret = serde_yaml::from_value::<Secret>(m.clone()).unwrap_or_default();
    //                 match secret.string_data {
    //                     Some(data) => data.len(),
    //                     None => match secret.data {
    //                         Some(data) => data.len(),
    //                         None => 0
    //                     }
    //                 }
    //             },
    //             None => 0
    //         };
    //
    //         let item = item.1.first().unwrap();
    //         println!(cols!(), item.name.namespace, item.name.name, "Opaque", data, age(item.created_at))
    //     }
    // }
}
