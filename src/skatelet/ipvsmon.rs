use crate::skate::exec_cmd;
use crate::util::NamespacedName;
use anyhow::anyhow;
use clap::Args;
use handlebars::Handlebars;
use k8s_openapi::api::core::v1::Service;
use log::info;
use serde_json::json;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use itertools::Itertools;

#[derive(Debug, Args)]
pub struct IpvsmonArgs {
    #[arg(long, long_help = "Name of the file to write keepalived config to.")]
    out: String,
    host: String,
    file: String,
}

pub fn ipvsmon(args: IpvsmonArgs) -> Result<(), Box<dyn Error>> {
    // args.service_name is fqn like foo.bar
    let mut manifest: Service = serde_yaml::from_str(&fs::read_to_string(args.file)?)?;
    let spec = manifest.spec.clone().unwrap_or_default();
    let name = spec.selector.unwrap_or_default().get("app.kubernetes.io/name").unwrap_or(&"default".to_string()).clone();
    if name == "" {
        return Err(anyhow!("service selector app.kubernetes.io/name is required").into());
    }
    let ns = manifest.metadata.namespace.unwrap_or("default".to_string());
    let fqn = NamespacedName { name, namespace: ns.clone() };
    manifest.metadata.namespace = Some(ns);


    let domain = format!("{}.pod.cluster.skate:80", fqn);
    // get all pod ips from dns <args.service_name>.cluster.skate
    info!("looking up ips for {}", &domain);
    let addrs: Vec<_> = domain.to_socket_addrs().unwrap_or_default()
        .map(|addr| addr.ip().to_string()).sorted().collect();


    let mut hasher = DefaultHasher::new();
    addrs.hash(&mut hasher);
    let new_hash = format!("{:x}", hasher.finish());
    let hash_file_name = format!("/run/skatelet-ipvsmon-{}.hash", fqn);

    let old_hash = fs::read_to_string(&hash_file_name).unwrap_or_default();

    // hashes match and output file exists
    if old_hash == new_hash && Path::new(&args.out).exists() {
        info!("ips haven't changed: {:?}", &addrs);
        return Ok(());
    }
    info!("ips changed, rewriting keepalived config for {} -> {:?}", &args.host, &addrs);

    fs::write(&hash_file_name, new_hash)?;


    // rewrite keepalived include file
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);

    handlebars.register_template_string("keepalived", include_str!("../resources/keepalived-service.conf")).map_err(|e| anyhow!(e).context("failed to load keepalived file"))?;

    // write config
    {
        let file = OpenOptions::new().write(true).create(true).truncate(true).open(args.out)?;
        handlebars.render_to_write("keepalived", &json!({
            "host": args.host,
            "manifest": manifest,
            "target_ips": addrs,
        }), file)?;
    }


    // reload keepalived
    let _ = exec_cmd("systemctl", &["reload", "keepalived"])?;
    Ok(())
}