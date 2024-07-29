use std::collections::HashMap;
use chrono::SecondsFormat;
use humantime;
use k8s_openapi::api::batch::v1::CronJob;
use crate::filestore::ObjectListItem;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::NamespacedName;

pub(crate) struct CronjobsLister {}

impl Lister<ObjectListItem> for CronjobsLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Option<Vec<ObjectListItem>> {
        si.cronjobs.as_ref().and_then(|jobs| Some(jobs.iter().filter(|j| {
            let filterable: Box<dyn NameFilters> = Box::new(*j);
            return filterable.filter_names(id, ns);
        }).map(|p| p.clone()).collect()))
    }

    fn print(&self, resources: Vec<ObjectListItem>) {
        println!(
            "{0: <10}  {1: <10}  {2: <10}  {3: <5}  {4: <10}  {5: <10}  {6: <10}  {7: <10}",
            "NAMESPACE", "NAME", "SCHEDULE", "TIMEZONE", "SUSPEND", "ACTIVE", "LAST SCHEDULE", "AGE"
        );
        let map = resources.iter().fold(HashMap::<NamespacedName, Vec<ObjectListItem>>::new(), |mut acc, item| {
            acc.entry(item.name.clone()).or_insert(vec![]).push(item.clone());
            acc
        });
        for (name, item) in map {
            let cronjob: CronJob = serde_yaml::from_value(item.first().as_ref().unwrap().manifest.as_ref().unwrap().clone()).unwrap_or_default();
            let spec = cronjob.spec.unwrap_or_default();
            let schedule = spec.schedule;
            let timezone = spec.time_zone;
            let created = item.first().unwrap().created_at;
            let age = match chrono::offset::Local::now().signed_duration_since(created).to_std() {
                Ok(age) => humantime::format_duration(age).to_string().split_whitespace().take(2).collect::<Vec<&str>>().join(""),
                Err(_) => "".to_string()
            };

            println!(
                "{0: <10}  {1: <10}  {2: <10}  {3: <5}  {4: <10}  {5: <10}  {6: <10}  {7: <10}",
                name.namespace, name.name, "TODO", timezone.unwrap_or("-".to_string()), "False", "-", "-", age
            )
        }
    }
}
