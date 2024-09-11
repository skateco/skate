use tabled::Tabled;
use crate::filestore::ObjectListItem;
use crate::get::{GetObjectArgs };
use crate::skatelet::{SystemInfo};
use crate::state::state::ClusterState;

pub(crate) trait NameFilters {
    fn id(&self) -> String;
    fn name(&self) -> String;
    fn namespace(&self) -> String;
    fn filter_names(&self, name: &str, ns: &str) -> bool {
        let ns = match ns.is_empty() {
            true => &"",
            false => ns
        };

        if !ns.is_empty() && self.namespace() != ns {
            return false;
        }
        if !name.is_empty() && (self.id() != name || self.name() != name) {
            return false;
        }
        if ns.is_empty() && name.is_empty() && self.namespace() == "skate" {
            return false;
        }
        return true;
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
    fn selector(&self, si: &SystemInfo, ns: &str, id: &str) -> Vec<T> where T: Tabled;
    fn list(&self, filters: &GetObjectArgs, state: &ClusterState) -> Vec<T> where T: Tabled {
        let ns = filters.namespace.clone().unwrap_or_default();
        let id = filters.id.clone().unwrap_or("".to_string());


        let resources = state.nodes.iter().map(|node| {
            match &node.host_info {
                Some(hi) => match &hi.system_info {
                    Some(si) => self.selector(&si, &ns, &id),
                    _ => vec![]
                }
                None => vec![]
            }
        }).flatten().collect();

        resources
    }
}
