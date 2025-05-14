use crate::filestore::ObjectListItem;
use crate::get::GetObjectArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::state::state::ClusterState;
use itertools::Itertools;
use tabled::Tabled;

pub(crate) trait NameFilters {
    fn id(&self) -> String {
        self.name()
    }
    fn name(&self) -> String;
    fn namespace(&self) -> String;
    fn filter_names(&self, name: &str, ns: &str) -> bool {
        let ns = match ns.is_empty() {
            true => "",
            false => ns,
        };

        if !ns.is_empty() && self.namespace() != ns {
            return false;
        }

        if !name.is_empty() && (self.id() != name && self.name() != name) {
            return false;
        }

        if ns.is_empty() && name.is_empty() && self.namespace() == "skate" {
            return false;
        }
        true
    }
}

impl NameFilters for &ObjectListItem {
    fn id(&self) -> String {
        self.name.to_string()
    }
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn namespace(&self) -> String {
        self.name.namespace.clone()
    }
}

pub(crate) trait Lister<T> {
    fn list(
        &self,
        resource_type: ResourceType,
        filters: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<T>
    where
        T: Tabled + NameFilters;
}

pub(crate) trait ResourceLister<T: From<ObjectListItem>> {
    fn list(
        &self,
        resource_type: ResourceType,
        filters: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<T>
    where
        T: Tabled + NameFilters,
    {
        let ns = filters.namespace.clone().unwrap_or_default();
        let id = filters.id.clone().unwrap_or("".to_string());

        let resources = state
            .catalogue(None, &[resource_type.clone()])
            .into_iter()
            .filter(|r| r.object.resource_type == resource_type)
            .filter(|r| r.object.filter_names(&id, &ns))
            .map(|r| r.object.clone().into())
            .collect();

        resources
    }
}
