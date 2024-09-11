use std::collections::HashMap;
use k8s_openapi::api::batch::v1::CronJob;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::{age, NamespacedName};
use tabled::{builder::Builder, settings::{style::Style}, Tabled};

pub(crate) struct CronjobsLister {}

#[derive(Tabled)]
#[tabled(rename_all = "UPPERCASE")]
pub struct CronListItem {
    pub namespace: String,
    pub name: String,
    pub schedule: String,
    pub timezone: String,
    pub suspend: String,
    pub active: String,
    pub last_schedule: String,
    pub age: String
}

impl Lister<CronListItem> for CronjobsLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<CronListItem> {
        si.cronjobs.as_ref().unwrap_or(&vec!()).iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);

        }).map(|item| {
            let item = item.clone();
            let cronjob: CronJob = serde_yaml::from_value(item.manifest.as_ref().unwrap().clone()).unwrap_or_default();
            let spec = cronjob.spec.unwrap_or_default();
            let schedule = spec.schedule;
            let timezone = spec.time_zone;
            let created = item.created_at;
            let age = age(created);
            CronListItem{
                namespace: item.name.namespace.clone(),
                name: item.name.name.clone(),
                schedule,
                timezone: timezone.unwrap_or("<none>".to_string()),
                suspend: "False".to_string(),
                active: "-".to_string(),
                last_schedule: "-".to_string(),
                age,
            }
        }).collect()
    }

    // TODO - record last run and how many running (somehow)
    // fn print(&self, resources: Vec<ObjectListItem>) {
    //
    //     let mut rows = vec!();
    //
    //     rows.push(["NAMESPACE", "NAME", "SCHEDULE", "TIMEZONE", "SUSPEND", "ACTIVE", "LAST_SCHEDULE", "AGE"].map(|i| i.to_string()));
    //
    //     let map = resources.iter().fold(HashMap::<NamespacedName, Vec<ObjectListItem>>::new(), |mut acc, item| {
    //         acc.entry(item.name.clone()).or_insert(vec![]).push(item.clone());
    //         acc
    //     });
    //     for (name, item) in map {
    //
    //         rows.push([name.namespace, name.name, schedule, timezone.unwrap_or("<none>".to_string()), "False".to_string(), "-".to_string(), "-".to_string(), age]);
    //     }
    //
    //     let mut table = Builder::from_iter(rows).build();
    //     table.with(Style::empty());
    //     println!("{}", table);
    //
    // }
}
