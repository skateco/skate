use std::collections::HashMap;
use chrono::SecondsFormat;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::networking::v1::Ingress;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::{age, NamespacedName};

pub(crate) struct IngresssLister {}

impl Lister<ObjectListItem> for IngresssLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.ingresses.as_ref().and_then(|jobs| Some(jobs.iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);
        }).map(|p| p.clone()).collect()))
    }

    fn print(&self, resources: Vec<ObjectListItem>) {
        macro_rules! cols {
            () => ("{0: <15}  {1: <15}  {2: <15}  {3: <25}  {4: <15}  {5: <15}  {6: <15}")
        }
        println!(
            cols!(),
            "NAMESPACE", "NAME", "CLASS", "HOSTS", "ADDRESS", "PORTS", "AGE"
        );
        let map = resources.iter().fold(HashMap::<NamespacedName, Vec<ObjectListItem>>::new(), |mut acc, item| {
            acc.entry(item.name.clone()).or_insert(vec![]).push(item.clone());
            acc
        });

        for (name, items) in map {
            let first = items.first().unwrap();
            let ingress: Ingress = serde_yaml::from_value(first.manifest.as_ref().unwrap().clone()).unwrap_or_default();
            let spec = ingress.spec.unwrap_or_default();

            let hosts = spec.rules.unwrap_or_default().iter().map(|r| r.host.clone().unwrap_or_default()).collect::<Vec<String>>().join(",");
            let age = age(first.created_at);
            let address = "";
            let class = "external";
            let ports = "80,443";
            println!(
                cols!(),
                name.namespace, name.name, class, hosts, address, ports, age
            )
        }
    }
}
