use crate::filestore::ObjectListItem;
use crate::get::lister::NameFilters;
use crate::get::Lister;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::util::age;
use k8s_openapi::api::batch::v1::CronJob;
use serde::Serialize;
use tabled::Tabled;

pub(crate) struct CronjobsLister {}

#[derive(Tabled, Serialize)]
#[tabled(rename_all = "UPPERCASE")]
pub struct CronListItem {
    #[serde(skip)]
    pub namespace: String,
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub schedule: String,
    #[serde(skip)]
    pub timezone: String,
    #[serde(skip)]
    pub suspend: String,
    #[serde(skip)]
    pub active: String,
    #[serde(skip)]
    pub last_schedule: String,
    #[serde(skip)]
    pub age: String,
    #[tabled(skip)]
    #[serde(flatten)]
    pub manifest: serde_yaml::Value,
}

impl From<ObjectListItem> for CronListItem {
    fn from(item: ObjectListItem) -> Self {
        let cronjob: CronJob =
            serde_yaml::from_value(item.manifest.as_ref().unwrap().clone()).unwrap_or_default();
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
            manifest: item.manifest.unwrap_or(serde_yaml::Value::Null),
        }
    }
}
impl NameFilters for CronListItem {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn namespace(&self) -> String {
        self.namespace.clone()
    }
}

// tests
#[cfg(test)]
mod tests {
    use crate::get::cronjob::CronListItem;
    use std::collections::BTreeMap;

    #[test]
    fn should_serialize() {
        let cronjob = CronListItem {
            namespace: "default".to_string(),
            name: "test-cronjob".to_string(),
            schedule: "*/5 * * * *".to_string(),
            timezone: "UTC".to_string(),
            suspend: "False".to_string(),
            active: "-".to_string(),
            last_schedule: "-".to_string(),
            age: "1m".to_string(),
            manifest: serde_yaml::to_value(BTreeMap::from([(
                "key".to_string(),
                "value".to_string(),
            )]))
            .unwrap(),
        };

        let yaml = serde_yaml::to_string(&cronjob).unwrap();
        assert_eq!(yaml, "key: value\n");
        let json = serde_json::to_string(&cronjob).unwrap();
        assert_eq!(json, "{\"key\":\"value\"}");
    }
}
