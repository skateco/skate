use crate::filestore::ObjectListItem;
use crate::get::GetObjectArgs;
use crate::skatelet::database::resource::ResourceType;
use crate::skatelet::SystemInfo;
use crate::state::state::ClusterState;
use itertools::Itertools;
use std::marker::PhantomData;
use tabled::Tabled;

pub(crate) trait NameFilters {
    fn id(&self) -> String {
        self.name()
    }
    fn name(&self) -> String;
    fn namespace(&self) -> String;

    fn matches_ns_name(&self, name: &str, ns: &str) -> bool {
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
        self.name.name.clone()
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

pub struct ResourceLister<T: Tabled + NameFilters + From<ObjectListItem>> {
    inner: PhantomData<T>,
}

impl<T: Tabled + NameFilters + From<ObjectListItem>> ResourceLister<T> {
    pub fn new() -> Self {
        ResourceLister { inner: PhantomData }
    }
}
impl<T: Tabled + NameFilters + From<ObjectListItem>> Lister<T> for ResourceLister<T> {
    fn list(
        &self,
        resource_type: ResourceType,
        filters: &GetObjectArgs,
        state: &ClusterState,
    ) -> Vec<T> {
        let ns = filters.namespace.clone().unwrap_or_default();
        let id = filters.id.clone().unwrap_or("".to_string());

        let resources = state
            .catalogue(None, &[resource_type.clone()])
            .into_iter()
            .filter(|r| r.object.resource_type == resource_type)
            .filter(|r| r.object.matches_ns_name(&id, &ns))
            .map(|r| r.object.clone().into())
            .collect();

        resources
    }
}
