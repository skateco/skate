use std::collections::BTreeMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use crate::skate::{Distribution, Platform};
use crate::skatelet::system::DiskInfo;
use crate::skatelet::SystemInfo;
use crate::ssh::HostInfo;
use crate::state::state::NodeState;
use crate::state::state::NodeStatus::Healthy;
use crate::util::NamespacedName;

pub fn node_state(name: &str) -> NodeState{
    NodeState{
        node_name: name.to_string(),
        status: Healthy,
        host_info: Some(HostInfo{
            node_name: name.to_string(),
            hostname: name.to_string(),
            platform: Platform{ arch: "x86_84".to_string(), distribution: Distribution::Ubuntu},
            skatelet_version: Some("1.0.0".to_string()),
            system_info: Some(SystemInfo{
                platform: Platform{ arch: "x86_84".to_string(), distribution: Distribution::Ubuntu},
                total_memory_mib: 1000,
                used_memory_mib: 0,
                total_swap_mib: 0,
                used_swap_mib: 0,
                num_cpus: 1,
                root_disk: Some(DiskInfo{
                    available_space_mib: 30_000,
                    total_space_mib: 40_000,
                    disk_kind: "ssd".to_string(),
                }),
                pods: None,
                ingresses: None,
                cronjobs: None,
                secrets: None,
                services: None,
                cluster_issuers: None,
                deployments: None,
                daemonsets: None,
                cpu_freq_mhz: 2,
                cpu_usage: 0.0,
                cpu_brand: "Intel".to_string(),
                cpu_vendor_id: "".to_string(),
                internal_ip_address: None,
                hostname: name.to_string(),
                cordoned: false,
            }),
            podman_version: Some("3.6.0".to_string()),
            ovs_version: Some("1.0.0".to_string()),
        }),
    }
}

impl From<NamespacedName> for ObjectMeta {
    fn from(ns_name: NamespacedName) -> Self {
        ObjectMeta{
            labels: Some(BTreeMap::from([
                ("skate.io/name".to_string(), ns_name.name),
                ("skate.io/namespace".to_string(), ns_name.namespace),
            ])),
            ..Default::default()
        }
    }
}