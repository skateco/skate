use std::collections::HashMap;
use chrono::SecondsFormat;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;

pub (crate) struct CronjobsLister {}

impl Lister<ObjectListItem> for CronjobsLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.cronjobs.as_ref().and_then(|jobs| Some(jobs.iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);
        }).map(|p| p.clone()).collect()))
    }

    fn print(&self, resources: Vec<ObjectListItem>) {
        println!(
            "{0: <30}  {1: <5}  {2: <20}",
            "NAME", "#", "CREATED",
        );
        let map = resources.iter().fold(HashMap::<String, Vec<ObjectListItem>>::new(), |mut acc, item| {
            acc.entry(item.name.to_string()).or_insert(vec![]).push(item.clone());
            acc
        });
        for (name, item) in map {
            println!(
                "{0: <30}  {1: <5}  {2: <20}",
                name, item.len(), item.first().unwrap().created_at.to_rfc3339_opts(SecondsFormat::Secs, true)
            )
        }
    }
}
