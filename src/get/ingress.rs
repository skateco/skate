use std::collections::HashMap;
use chrono::SecondsFormat;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::{age, NamespacedName};

pub(crate) struct IngresssLister {}

impl Lister<ObjectListItem> for IngresssLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.ingresses.as_ref().and_then(|jobs| Some(jobs.iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(j.clone());
            return filterable.filter_names(id, ns);
        }).map(|p| p.clone()).collect()))
    }

    fn print(&self, resources: Vec<ObjectListItem>) {
        macro_rules! cols {
            () => ("{0: <15}  {1: <15}  {2: <15}  {3: <15}  {4: <15}  {5: <15}  {6: <15}")
        }
        println!(
            cols!(),
            "NAMESPACE", "NAME", "CLASS", "HOSTS", "ADDRESS", "PORTS", "AGE"
        );
        let map = resources.iter().fold(HashMap::<NamespacedName, Vec<ObjectListItem>>::new(), |mut acc, item| {
            acc.entry(item.name.clone()).or_insert(vec![]).push(item.clone());
            acc
        });

        for (name, item) in map {
            let hosts = "TODO";
            let age = age(item.first().unwrap().created_at);
            let address = "TODO";
            let class = "TODO";
            let ports = "TODO";
            println!(
                cols!(),
                name.namespace, name.name, class, hosts, address, ports, age
            )
        }
    }
}
