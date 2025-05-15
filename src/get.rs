mod cronjob;
mod daemonset;
mod deployment;
mod ingress;
mod lister;
mod node;
mod pod;
mod secret;
mod service;

use crate::config::Config;
use crate::refresh::Refresh;
use clap::{Args, Subcommand};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use strum_macros::EnumString;
use tabled::settings::Style;
use tabled::{Table, Tabled};

use crate::skate::ConfigFileArgs;

use crate::deps::{SshManager, With};
use crate::errors::SkateError;
use crate::filestore::ObjectListItem;
use crate::get::cronjob::CronListItem;
use crate::get::daemonset::DaemonsetLister;
use crate::get::deployment::DeploymentLister;
use crate::get::ingress::IngressListItem;
use crate::get::lister::{Lister, NameFilters, ResourceLister};
use crate::get::node::NodeLister;
use crate::get::pod::PodLister;
use crate::get::secret::SecretListItem;
use crate::get::service::ServiceListItem;
use crate::refresh;
use crate::skatelet::database::resource::ResourceType;

#[derive(Debug, Clone, Args)]
pub struct GetArgs {
    #[command(subcommand)]
    commands: GetCommands,
}

#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum OutputFormat {
    Json,
    Yaml,
    Name,
}

struct GetListV1<'a, T: Serialize> {
    pub items: &'a Vec<T>,
}

impl<'a, T: Serialize> Serialize for GetListV1<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("GetListV1", 3)?;
        state.serialize_field("apiVersion", "v1")?;
        state.serialize_field("kind", "List")?;
        state.serialize_field("items", &self.items)?;
        state.end()
    }
}

impl OutputFormat {
    pub fn print_one<T: Tabled + Serialize>(self, object: &T) {
        match self {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(object).unwrap();
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(object).unwrap();
                println!("{}", yaml);
            }
            OutputFormat::Name => {
                let mut table = Table::new(vec![object]);
                table.with(Style::empty());
                println!("{}", table);
            }
        }
    }
    pub fn print_many<T: Tabled + Serialize>(self, objects: &Vec<T>) {
        match self {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&GetListV1 { items: objects }).unwrap();
                println!("{}", json);
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(&GetListV1 { items: objects }).unwrap();
                println!("{}", yaml);
            }
            OutputFormat::Name => {
                let mut table = Table::new(objects);
                table.with(Style::empty());
                println!("{}", table);
            }
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct GetObjectArgs {
    #[command(flatten)]
    config: ConfigFileArgs,
    #[arg(long, short, long_help = "Filter by resource namespace")]
    namespace: Option<String>,
    #[arg()]
    id: Option<String>,
    #[arg(long, short, long_help = "Output format. One of: (json, yaml, name)")]
    output: Option<OutputFormat>,
}

#[derive(Clone, Debug, Subcommand)]
pub enum GetCommands {
    #[command(alias("pods"))]
    Pod(GetObjectArgs),
    #[command(alias("deployments"))]
    Deployment(GetObjectArgs),
    #[command(alias("daemonsets"))]
    Daemonset(GetObjectArgs),
    #[command(alias("nodes"))]
    Node(GetObjectArgs),
    #[command()]
    Ingress(GetObjectArgs),
    #[command(alias("cronjobs"))]
    Cronjob(GetObjectArgs),
    #[command(alias("secrets"))]
    Secret(GetObjectArgs),
    #[command(alias("services"))]
    Service(GetObjectArgs),
}

pub trait GetDeps: With<dyn SshManager> {}

pub struct Get<D: GetDeps> {
    pub deps: D,
}

impl<D: GetDeps + refresh::RefreshDeps> Get<D> {
    pub async fn get(&self, args: GetArgs) -> Result<(), SkateError> {
        let global_args = args.clone();
        match args.commands {
            GetCommands::Pod(args) => self.get_pod(global_args, args).await,
            GetCommands::Deployment(args) => self.get_deployment(global_args, args).await,
            GetCommands::Daemonset(args) => self.get_daemonsets(global_args, args).await,
            GetCommands::Node(args) => self.get_nodes(global_args, args).await,
            GetCommands::Ingress(args) => self.get_ingress(global_args, args).await,
            GetCommands::Cronjob(args) => self.get_cronjobs(global_args, args).await,
            GetCommands::Secret(args) => self.get_secrets(global_args, args).await,
            GetCommands::Service(args) => self.get_services(global_args, args).await,
        }
    }

    async fn get_resource_objects<
        T: Tabled + NameFilters + serde::Serialize + From<ObjectListItem>,
    >(
        &self,
        resource_type: ResourceType,
        _global_args: GetArgs,
        args: GetObjectArgs,
        lister: impl Lister<T>,
    ) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;
        let mgr = self.deps.get();
        let (conns, errors) = mgr
            .cluster_connect(config.active_cluster(args.config.context.clone())?)
            .await;
        if errors.is_some() {
            eprintln!("{}", errors.unwrap())
        }

        if conns.is_none() {
            return Ok(());
        }

        let conns = conns.unwrap();

        let state = Refresh::<D>::refreshed_state(
            &config.current_context.clone().unwrap_or("".to_string()),
            &conns,
            &config,
        )
        .await?;

        let objects = lister.list(resource_type, &args, &state);

        if objects.is_empty() {
            if args.namespace.is_some() {
                println!(
                    "No resources found for namespace {}",
                    args.namespace.unwrap()
                );
            } else {
                println!("No resources found");
            }
            return Ok(());
        }

        let output_format = args.output.unwrap_or(OutputFormat::Name);

        if args.id.is_some() && objects.len() == 1 {
            output_format.print_one(&objects[0]);
        } else {
            output_format.print_many(&objects);
        }
        Ok(())
    }

    // TODO - move everything in as a resource and remove this
    async fn get_objects<T: Tabled + NameFilters + serde::Serialize>(
        &self,
        resource_type: ResourceType,
        _global_args: GetArgs,
        args: GetObjectArgs,
        lister: &dyn Lister<T>,
    ) -> Result<(), SkateError> {
        let config = Config::load(Some(args.config.skateconfig.clone()))?;
        let mgr = self.deps.get();
        let (conns, errors) = mgr
            .cluster_connect(config.active_cluster(args.config.context.clone())?)
            .await;
        if errors.is_some() {
            eprintln!("{}", errors.unwrap())
        }

        if conns.is_none() {
            return Ok(());
        }

        let conns = conns.unwrap();

        let state = Refresh::<D>::refreshed_state(
            &config.current_context.clone().unwrap_or("".to_string()),
            &conns,
            &config,
        )
        .await?;

        let objects = lister.list(resource_type, &args, &state);

        if objects.is_empty() {
            if args.namespace.is_some() {
                println!(
                    "No resources found for namespace {}",
                    args.namespace.unwrap()
                );
            } else {
                println!("No resources found");
            }
            return Ok(());
        }

        let output_format = args.output.unwrap_or(OutputFormat::Name);

        if args.id.is_some() && objects.len() == 1 {
            output_format.print_one(&objects[0]);
        } else {
            output_format.print_many(&objects);
        }
        Ok(())
    }

    async fn get_deployment(
        &self,
        global_args: GetArgs,
        args: GetObjectArgs,
    ) -> Result<(), SkateError> {
        let lister = DeploymentLister {};
        self.get_objects(ResourceType::Deployment, global_args, args, &lister)
            .await
    }

    async fn get_daemonsets(
        &self,
        global_args: GetArgs,
        args: GetObjectArgs,
    ) -> Result<(), SkateError> {
        let lister = DaemonsetLister {};
        self.get_objects(ResourceType::DaemonSet, global_args, args, &lister)
            .await
    }

    async fn get_pod(&self, global_args: GetArgs, args: GetObjectArgs) -> Result<(), SkateError> {
        let lister = PodLister {};
        self.get_objects(ResourceType::Pod, global_args, args, &lister)
            .await
    }

    async fn get_ingress(
        &self,
        global_args: GetArgs,
        args: GetObjectArgs,
    ) -> Result<(), SkateError> {
        let lister = ResourceLister::<IngressListItem>::new();
        self.get_resource_objects(ResourceType::Ingress, global_args, args, lister)
            .await
    }

    async fn get_cronjobs(
        &self,
        global_args: GetArgs,
        args: GetObjectArgs,
    ) -> Result<(), SkateError> {
        let lister = ResourceLister::<CronListItem>::new();
        self.get_resource_objects(ResourceType::CronJob, global_args, args, lister)
            .await
    }

    async fn get_nodes(&self, global_args: GetArgs, args: GetObjectArgs) -> Result<(), SkateError> {
        let lister = NodeLister {};
        self.get_objects(ResourceType::Node, global_args, args, &lister)
            .await
    }

    async fn get_secrets(
        &self,
        global_args: GetArgs,
        args: GetObjectArgs,
    ) -> Result<(), SkateError> {
        let lister = ResourceLister::<SecretListItem>::new();
        self.get_resource_objects(ResourceType::Secret, global_args, args, lister)
            .await
    }

    async fn get_services(
        &self,
        global_args: GetArgs,
        args: GetObjectArgs,
    ) -> Result<(), SkateError> {
        let lister = ResourceLister::<ServiceListItem>::new();
        self.get_resource_objects(ResourceType::Service, global_args, args, lister)
            .await
    }
}
