use std::collections::HashMap;
use k8s_openapi::api::core::v1::Secret;


use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::{SystemInfo};

use crate::util::age;

pub(crate) struct SecretLister {}

impl Lister<ObjectListItem> for SecretLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.secrets.as_ref().and_then(|secrets| Some(secrets.iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);
        }).map(|p| p.clone()).collect()))
    }

    fn print(&self, items: Vec<ObjectListItem>) {
        let map = items.iter().fold(HashMap::<String, Vec<ObjectListItem>>::new(), |mut acc, item| {
            acc.entry(item.name.to_string()).or_insert(vec![]).push(item.clone());
            acc
        });

        macro_rules! cols {
            () => ("{0: <15}  {1: <15}  {2: <15}  {3: <15}  {4: <10}")
        }
        println!(
            cols!(),
            "NAMESPACE", "NAME", "TYPE", "DATA", "AGE",
        );


        for item in map {

            let data: usize = match item.1.first().unwrap().manifest{
                Some(ref m) => {
                    let secret = serde_yaml::from_value::<Secret>(m.clone()).unwrap_or_default();
                    match secret.string_data {
                        Some(data) => data.len(),
                        None => match secret.data {
                            Some(data) => data.len(),
                            None => 0
                        }
                    }
                },
                None => 0
            };

            let item = item.1.first().unwrap();
            println!(cols!(), item.name.namespace, item.name.name, "Opaque", data, age(item.created_at))
        }
    }
}
