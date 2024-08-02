use std::collections::HashMap;
use chrono::{Local, SecondsFormat};
use itertools::Itertools;
use crate::filestore::ObjectListItem;
use crate::get::{GetObjectArgs, IdCommand, Lister};
use crate::skatelet::{PodmanPodInfo, PodmanPodStatus, SystemInfo};
use crate::state::state::ClusterState;
use crate::util::age;

pub(crate) struct SecretLister {}

impl Lister<ObjectListItem> for SecretLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.secrets.clone()
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

        // TODO - get from manifest
        let data = 1;

        for item in map {
            let item = item.1.first().unwrap();
            println!(cols!(), item.name.namespace, item.name.name, "Opaque", data, age(item.created_at))
        }
    }
}
