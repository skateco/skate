#![allow(unused)]

use std::error::Error;
use async_trait::async_trait;
use clap::{Args, Command, Parser, Subcommand};
use k8s_openapi::{List, Metadata, NamespaceResourceScope, Resource, ResourceScope};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{Container, Pod, PodTemplateSpec, Secret, Service};
use serde_yaml;
use serde::{Deserialize, Serialize};
use tokio;
use crate::apply::{apply, ApplyArgs};
use crate::refresh::{refresh, RefreshArgs};
use async_ssh2_tokio::client::{AuthMethod, Client, CommandExecutedResult, ServerCheckMethod};
use async_ssh2_tokio::Error as SshError;
use strum_macros::{Display, EnumString};
use std::{fs, io, process};
use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::env::var;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir, File, metadata};
use std::io::Read;
use std::ops::Deref;
use std::path::Path;
use std::time::{Duration, SystemTime};
use path_absolutize::*;
use anyhow::anyhow;
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use log::error;
use serde_yaml::{Error as SerdeYamlError, Value};
use crate::{config, spec};
use crate::config::{cache_dir, Config, Node};
use crate::config_cmd::ConfigArgs;
use crate::create::{create, CreateArgs};
use crate::delete::{delete, DeleteArgs};
use crate::get::{get, GetArgs};
use crate::describe::{DescribeArgs, describe};
use crate::logs::{LogArgs, logs};
use crate::skate::Distribution::{Debian, Raspbian, Ubuntu, Unknown};
use crate::skate::Os::{Darwin, Linux};
use crate::spec::cert::ClusterIssuer;
use crate::ssh::{SshClient, SshClients};
use crate::state::state::NodeState;
use crate::util::{metadata_name, NamespacedName, slugify, TARGET};


#[derive(Debug, Parser)]
#[command(name = "skate")]
#[command(about = "Skate CLI", long_about = None, arg_required_else_help = true, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Create(CreateArgs),
    Delete(DeleteArgs),
    Apply(ApplyArgs),
    Refresh(RefreshArgs),
    Get(GetArgs),
    Describe(DescribeArgs),
    Logs(LogArgs),
    Config(ConfigArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigFileArgs {
    #[arg(long, long_help = "Configuration for skate.", default_value = "~/.skate/config.yaml")]
    pub skateconfig: String,
    #[arg(long, long_help = "Name of the context to use.")]
    pub context: Option<String>,
}

pub async fn skate() -> Result<(), Box<dyn Error>> {
    config::ensure_config();
    let args = Cli::parse();
    match args.command {
        Commands::Create(args) => create(args).await,
        Commands::Delete(args) => delete(args).await,

        Commands::Apply(args) => apply(args).await,
        Commands::Refresh(args) => refresh(args).await,
        Commands::Get(args) => get(args).await,
        Commands::Describe(args) => describe(args).await,
        Commands::Logs(args) => logs(args).await,
        Commands::Config(args) => crate::config_cmd::config(args),
        _ => Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Display, Clone, EnumString)]
pub enum ResourceType {
    Pod,
    Deployment,
    DaemonSet,
    Ingress,
    CronJob,
    Secret,
    Service,
    ClusterIssuer,
}

#[derive(Debug, Serialize, Deserialize, Display, Clone)]
pub enum SupportedResources {
    #[strum(serialize = "Pod")]
    Pod(Pod),
    #[strum(serialize = "Deployment")]
    Deployment(Deployment),
    #[strum(serialize = "DaemonSet")]
    DaemonSet(DaemonSet),
    #[strum(serialize = "Ingress")]
    Ingress(Ingress),
    #[strum(serialize = "CronJob")]
    CronJob(CronJob),
    #[strum(serialize = "Secret")]
    Secret(Secret),
    #[strum(serialize = "Service")]
    Service(Service),
    #[strum(serialize = "ClusterIssuer")]
    ClusterIssuer(ClusterIssuer),

}

impl TryFrom<Value> for SupportedResources {
    type Error = Box<dyn Error>;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let api_version_key = Value::String("apiVersion".to_owned());
        let kind_key = Value::String("kind".to_owned());

        let api_version = value.get(&api_version_key).and_then(Value::as_str);
        let kind = value.get(&kind_key).and_then(Value::as_str);
        match (api_version, kind) {
            (Some(api_version), Some(kind)) => {
                if api_version == Pod::API_VERSION &&
                    kind == Pod::KIND
                {
                    let pod: Pod = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::Pod(pod))
                } else if api_version == Deployment::API_VERSION &&
                    kind == Deployment::KIND
                {
                    let deployment: Deployment = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::Deployment(deployment))
                } else if api_version == DaemonSet::API_VERSION &&
                    kind == DaemonSet::KIND
                {
                    let daemonset: DaemonSet = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::DaemonSet(daemonset))
                } else if api_version == Ingress::API_VERSION && kind == Ingress::KIND
                {
                    let ingress: Ingress = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::Ingress(ingress))
                } else if
                api_version == CronJob::API_VERSION &&
                    kind == CronJob::KIND
                {
                    let cronjob: CronJob = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::CronJob(cronjob))
                } else if
                api_version == Secret::API_VERSION &&
                    kind == Secret::KIND
                {
                    let secret: Secret = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::Secret(secret))
                } else if
                api_version == Service::API_VERSION &&
                    kind == Service::KIND
                {
                    let service: Service = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::Service(service))
                } else if
                api_version == ClusterIssuer::API_VERSION &&
                    kind == ClusterIssuer::KIND {
                    let clusterissuer: ClusterIssuer = serde::Deserialize::deserialize(value)?;
                    Ok(SupportedResources::ClusterIssuer(clusterissuer))
                } else {
                    Err(anyhow!(format!("version: {}, kind {}", api_version, kind)).context("unsupported resource type").into())
                }
            }
            _ => {
                Err(anyhow!("missing 'kind' and 'apiVersion' fields").context("unsupported resource type").into())
            }
        }
    }
}

impl SupportedResources {
    pub fn name(&self) -> NamespacedName {
        match self {
            SupportedResources::Pod(r) => metadata_name(r),
            //SupportedResources::Pod(r) => NamespacedName{name: r.metadata.name.clone().unwrap_or_default(), namespace: r.metadata.labels.clone().unwrap_or_default().get("skate.io/namespace").unwrap_or(&"default".to_string())},
            SupportedResources::Deployment(r) => metadata_name(r),
            SupportedResources::DaemonSet(r) => metadata_name(r),
            SupportedResources::Ingress(r) => metadata_name(r),
            SupportedResources::CronJob(r) => metadata_name(r),
            SupportedResources::Secret(s) => metadata_name(s),
            SupportedResources::Service(s) => metadata_name(s),
            SupportedResources::ClusterIssuer(c) => metadata_name(c),
        }
    }

    pub async fn pre_remove_hook(&self, node: &NodeState, conns: &SshClients) -> Result<(), Box<dyn Error>> {
        match self {
            SupportedResources::Pod(pod) => {
                let mut errs = vec!();
                // remove the pod ip from dns on deployed node
                let ips: Vec<_> = match conns.find(&node.node_name).unwrap()
                    .execute(&format!("sudo skatelet dns remove --pod-id {}", &pod.metadata.name.clone().unwrap())).await {
                    Ok((ips)) => {
                        let ips: Vec<_> = ips.lines().map(|l| l.to_string()).collect();
                        ips
                    }
                    Err(e) => {
                        errs.push(e);
                        vec!()
                    }
                };

                let now = chrono::Utc::now().timestamp();

                let labels = pod.metadata.labels.as_ref().ok_or("no labels")?;

                let name = metadata_name(pod);
                let deployment = labels.get("skate.io/deployment");
                if deployment.is_none() {
                    return Ok(());
                }
                let deployment = deployment.unwrap().clone();
                let fq_deployment_name = NamespacedName { name: deployment, namespace: name.namespace };


                let cmd = format!(r#"sudo skatelet ipvs disable-ip {} {} && sudo $(systemctl cat skate-ipvsmon-{}.service|grep ExecStart|sed 's/ExecStart=//')"#, &fq_deployment_name, ips.join(" "), &fq_deployment_name);
                let res = conns.execute(&cmd).await;
                res.into_iter().for_each(|(node, result)| {
                    if result.is_err() {
                        let err = result.err().unwrap();
                        errs.push(err);
                    }
                });

                if !errs.is_empty() {
                    return Err(anyhow!(errs.iter().map(|e|e.to_string()).collect::<Vec<String>>().join(". ")).context("failed to run pre-remove hook").into());
                }

                Ok(())
            }
            _ => Ok(())
        }
    }

    // whether there's host network set
    pub fn host_network(&self) -> bool {
        match self {
            SupportedResources::Pod(p) => p.clone().spec.unwrap_or_default().host_network.unwrap_or_default(),
            SupportedResources::Deployment(d) => d.clone().spec.unwrap_or_default().template.spec.unwrap_or_default().host_network.unwrap_or_default(),
            SupportedResources::DaemonSet(d) => d.clone().spec.unwrap_or_default().template.spec.unwrap_or_default().host_network.unwrap_or_default(),
            SupportedResources::Ingress(_) => false,
            SupportedResources::CronJob(c) => c.clone().spec.unwrap_or_default().job_template.spec.unwrap_or_default().template.spec.unwrap_or_default().host_network.unwrap_or_default(),
            SupportedResources::Secret(_) => false,
            SupportedResources::Service(_) => false,
            SupportedResources::ClusterIssuer(_) => false,
        }
    }
    fn fixup_pod_template(template: PodTemplateSpec, ns: &str) -> Result<PodTemplateSpec, Box<dyn Error>> {
        let mut template = template.clone();
        // the secret names have to be suffixed with .<namespace> in order for them not to be available across namespace
        template.spec = match template.spec {
            Some(ref mut spec) => {
                // first do env-var secrets
                spec.containers = spec.containers.clone().into_iter().map(|mut container| {
                    container.env = match container.env {
                        Some(env_list) => {
                            Some(env_list.into_iter().map(|mut e| {
                                let name_opt = e.value_from.as_ref().and_then(|v| v.secret_key_ref.clone()).and_then(|s| s.name);
                                if name_opt.is_some() {
                                    e.value_from.as_mut().unwrap().secret_key_ref.as_mut().unwrap().name = Some(format!("{}.{}", &name_opt.unwrap(), &ns));
                                }
                                e
                            }).collect())
                        }
                        None => None
                    };
                    container
                }).collect();
                // now do volume secrets
                spec.volumes = spec.volumes.clone().and_then(|volumes| Some(volumes.into_iter().map(|mut volume| {
                    volume.secret = volume.secret.clone().map(|mut secret| {
                        secret.secret_name = secret.secret_name.clone().and_then(|secret_name| Some(format!("{}.{}", secret_name, ns)));
                        secret
                    });
                    volume
                }).collect()));


                Some(spec.clone())
            }
            None => None
        };

        Ok(template)
    }

    fn fixup_metadata(meta: ObjectMeta, extra_labels: Option<HashMap<String, String>>) -> Result<ObjectMeta, Box<dyn Error>> {
        let mut meta = meta.clone();
        let ns = meta.namespace.clone().unwrap_or("default".to_string());
        let name = meta.name.clone().unwrap();

        // labels apply to both pods and containers
        let mut labels = meta.labels.unwrap_or_default();
        labels.insert("skate.io/name".to_string(), name.clone());
        labels.insert("skate.io/namespace".to_string(), ns.clone());

        match extra_labels {
            Some(extra_labels) => labels.extend(extra_labels),
            _ => {}
        };
        meta.labels = Some(labels);

        let mut annotations = meta.annotations.unwrap_or_default();
        annotations.insert("io.skate".to_string(), "true".to_string());
        meta.annotations = Some(annotations);

        Ok(meta)
    }

    // TODO - do we need this? scheduler does most of this
    pub fn fixup(self) -> Result<Self, Box<dyn Error>> {
        let mut resource = self.clone();
        let resource = match resource {
            SupportedResources::Secret(ref mut s) => {
                let original_name = s.metadata.name.clone().unwrap_or("".to_string());
                if original_name.is_empty() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if s.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }

                s.metadata = Self::fixup_metadata(s.metadata.clone(), None)?;
                s.metadata.name = Some(format!("{}.{}", original_name, s.metadata.namespace.clone().unwrap()));
                resource
            }
            SupportedResources::CronJob(ref mut c) => {
                let original_name = c.metadata.name.clone().unwrap_or("".to_string());
                if original_name.is_empty() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if c.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }

                let mut extra_labels = HashMap::from([
                    ("skate.io/cronjob".to_string(), original_name)
                ]);
                c.metadata = Self::fixup_metadata(c.metadata.clone(), None)?;
                c.spec = match c.spec.clone() {
                    Some(mut spec) => {
                        match spec.job_template.spec {
                            Some(mut job_spec) => {
                                job_spec.template.metadata = {
                                    let mut meta = job_spec.template.metadata.clone().unwrap_or_default();
                                    // forward the namespace
                                    meta.namespace = c.metadata.namespace.clone();
                                    // if no name is set, set it to the cronjob name
                                    if meta.name.is_none() {
                                        meta.name = Some(c.metadata.name.clone().unwrap());
                                    }
                                    let meta = Self::fixup_metadata(meta, Some(extra_labels))?;
                                    Some(meta)
                                };

                                job_spec.template = Self::fixup_pod_template(job_spec.template.clone(), c.metadata.namespace.as_ref().unwrap())?;
                                spec.job_template.spec = Some(job_spec);
                                Some(spec)
                            }
                            None => None
                        }
                    }
                    None => None
                };
                resource
            }
            SupportedResources::Ingress(ref mut i) => {
                let original_name = i.metadata.name.clone().unwrap_or("".to_string());
                if i.metadata.name.is_none() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if i.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }

                let mut extra_labels = HashMap::from([]);

                i.metadata = Self::fixup_metadata(i.metadata.clone(), Some(extra_labels))?;
                // set name to be name.namespace
                i.metadata.name = Some(format!("{}", metadata_name(i)));
                resource
            }
            SupportedResources::Pod(ref mut p) => {
                if p.metadata.name.is_none() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if p.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }
                p.metadata = Self::fixup_metadata(p.metadata.clone(), None)?;
                // set name to be name.namespace
                p.metadata.name = Some(format!("{}", metadata_name(p)));
                // go through
                resource
            }
            SupportedResources::Deployment(ref mut d) => {
                let original_name = d.metadata.name.clone().unwrap_or("".to_string());
                if original_name.is_empty() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if d.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }

                let mut extra_labels = HashMap::from([
                    ("skate.io/deployment".to_string(), original_name.clone())
                ]);
                d.metadata = Self::fixup_metadata(d.metadata.clone(), Some(extra_labels.clone()))?;

                d.spec = match d.spec.clone() {
                    Some(mut spec) => {
                        spec.template.metadata = {
                            let mut meta = spec.template.metadata.clone().unwrap_or_default();
                            // forward the namespace
                            meta.namespace = d.metadata.namespace.clone();
                            if meta.name.clone().unwrap_or_default().is_empty() {
                                meta.name = Some(original_name.clone());
                            }
                            let meta = Self::fixup_metadata(meta, Some(extra_labels))?;
                            Some(meta)
                        };

                        spec.template = Self::fixup_pod_template(spec.template.clone(), d.metadata.namespace.as_ref().unwrap())?;
                        Some(spec)
                    }
                    None => None
                };
                resource
            }
            SupportedResources::DaemonSet(ref mut ds) => {
                let original_name = ds.metadata.name.clone().unwrap_or("".to_string());
                if original_name.is_empty() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if ds.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }

                let mut extra_labels = HashMap::from([
                    ("skate.io/daemonset".to_string(), original_name.clone())
                ]);
                ds.metadata = Self::fixup_metadata(ds.metadata.clone(), None)?;
                ds.spec = match ds.spec.clone() {
                    Some(mut spec) => {
                        spec.template.metadata = {
                            let mut meta = spec.template.metadata.clone().unwrap();
                            // forward the namespace
                            meta.namespace = ds.metadata.namespace.clone();
                            if meta.name.clone().unwrap_or_default().is_empty() {
                                meta.name = Some(original_name.clone());
                            }
                            let meta = Self::fixup_metadata(meta, Some(extra_labels))?;
                            Some(meta)
                        };

                        spec.template = Self::fixup_pod_template(spec.template.clone(), ds.metadata.namespace.as_ref().unwrap())?;
                        Some(spec)
                    }
                    None => None
                };
                resource
            }
            SupportedResources::Service(ref mut s) => {
                let original_name = s.metadata.name.clone().unwrap_or("".to_string());
                if s.metadata.name.is_none() {
                    return Err(anyhow!("metadata.name is empty").into());
                }
                if s.metadata.namespace.is_none() {
                    return Err(anyhow!("metadata.namespace is empty").into());
                }


                s.metadata = Self::fixup_metadata(s.metadata.clone(), None)?;
                // set name to be name.namespace
                s.metadata.name = Some(format!("{}", metadata_name(s)));
                resource
            }
            SupportedResources::ClusterIssuer(ref mut issuer) => {
                let original_name = issuer.metadata.name.clone().unwrap_or("".to_string());

                issuer.metadata = Self::fixup_metadata(issuer.metadata.clone(), None)?;
                issuer.metadata.name = Some(format!("{}", metadata_name(issuer)));
                resource
            }
        };
        Ok(resource)
    }
}


pub fn read_manifests(filenames: Vec<String>) -> Result<Vec<SupportedResources>, Box<dyn Error>> {
    let api_version_key = Value::String("apiVersion".to_owned());
    let kind_key = Value::String("kind".to_owned());

    let mut result: Vec<SupportedResources> = Vec::new();

    let num_filenames = filenames.len();

    let supported_resources =

        for filename in filenames {
            let str_file = {
                if num_filenames == 1 && filename == "-" {
                    let mut stdin = io::stdin();
                    let mut buffer = String::new();
                    stdin.read_to_string(&mut buffer)?;
                    buffer
                } else {
                    fs::read_to_string(filename).expect("failed to read file")
                }
            };
            for document in serde_yaml::Deserializer::from_str(&str_file) {
                let value = Value::deserialize(document).expect("failed to read document");
                if let Value::Mapping(mapping) = &value {
                    result.push(SupportedResources::try_from(value)?)
                }
            }
        };
    Ok(result)
}

#[derive(Debug, Display, EnumString, Clone, Serialize, Deserialize)]
pub enum Os {
    Unknown,
    Linux,
    Darwin,
}

impl Os {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase() {
            s if s.contains("linux") => Os::Linux,
            s if s.contains("darwin") => Os::Darwin,
            _ => Os::Unknown
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub arch: String,
    pub distribution: Distribution,
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("arch: {}, distribution: {}", self.arch, self.distribution))
    }
}

impl Platform {
    pub fn target() -> Self {
        let parts: Vec<&str> = TARGET.split('-').collect();


        let arch = parts.first().expect("failed to find arch");

        let issue = fs::read_to_string("/etc/issue").expect("failed to read /etc/issue");
        let distro = issue.split_whitespace().next().expect("no distribution found in /etc/issue");

        return Platform { arch: arch.to_string(), distribution: Distribution::from(distro) };
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Display)]
pub enum Distribution {
    Unknown,
    Debian,
    Raspbian,
    Ubuntu,
}

impl From<&str> for Distribution {
    fn from(s: &str) -> Self {
        match s.to_lowercase() {
            s if s.starts_with("debian") => Debian,
            s if s.starts_with("raspbian") => Raspbian,
            s if s.starts_with("ubuntu") => Ubuntu,
            _ => Unknown
        }
    }
}


pub(crate) fn exec_cmd(command: &str, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = process::Command::new(command)
        .args(args)
        .output().map_err(|e| anyhow!(e).context("failed to run command"))?;
    if !output.status.success() {
        return Err(anyhow!("exit code {}, stderr: {}", output.status, String::from_utf8_lossy(&output.stderr).to_string()).context(format!("{} {} failed", command, args.join(" "))).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
}



