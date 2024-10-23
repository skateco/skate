use k8s_openapi::api::batch::v1::CronJob;
use crate::get::{Lister};
use crate::get::lister::NameFilters;
use crate::skatelet::SystemInfo;
use crate::util::age;
use tabled::Tabled;

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
    pub age: String,
}

impl NameFilters for CronListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}

impl Lister<CronListItem> for CronjobsLister {
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<CronListItem> {
        si.cronjobs.as_ref().unwrap_or(&vec!()).iter().filter(|j| {
            j.filter_names(id, ns)
        }).map(|item| {
            let item = item.clone();
            let cronjob: CronJob = serde_yaml::from_value(item.manifest.as_ref().unwrap().clone()).unwrap_or_default();
            let spec = cronjob.spec.unwrap_or_default();
            let schedule = spec.schedule;
            let timezone = spec.time_zone;
            let created = item.created_at;
            let age = age(created);
            CronListItem {
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
}
