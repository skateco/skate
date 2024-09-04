use std::collections::HashMap;
use k8s_openapi::api::core::v1::Service;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::{age, NamespacedName};

pub(crate) struct ServiceLister {}

impl Lister<ObjectListItem> for ServiceLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.services.as_ref().and_then(|jobs| Some(jobs.iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);
        }).map(|p| p.clone()).collect()))
    }

    fn print(&self, resources: Vec<ObjectListItem>) {
        macro_rules! cols {
            () => ("{0: <15}  {1: <15}  {2: <15}  {3: <15}  {4: <15}  {5: <25}  {6: <7}")
        }
        println!(
            cols!(),
            "NAMESPACE", "NAME", "TYPE", "CLUSTER-IP", "EXTERNAL-IP", "PORT(S)", "AGE"
        );
        let map = resources.iter().fold(HashMap::<NamespacedName, Vec<ObjectListItem>>::new(), |mut acc, item| {
            acc.entry(item.name.clone()).or_insert(vec![]).push(item.clone());
            acc
        });

        for (name, items) in map {
            let first = items.first().unwrap();
            let ingress: Service = serde_yaml::from_value(first.manifest.as_ref().unwrap().clone()).unwrap_or_default();
            let spec = ingress.spec.unwrap_or_default();
            let ports: Vec<_> = spec.ports.unwrap_or_default().into_iter().map(|p|p.port.to_string()).collect();

            // TODO get the cluster ip

            let age = age(first.created_at);
            println!(
                cols!(),
                name.namespace, name.name, "ClusterIP", "-", "-", ports.join(",") , age
            )
        }
    }
}
