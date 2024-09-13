use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use anyhow::anyhow;
use chrono::{DateTime, Local};
use deunicode::deunicode_char;
use fs2::FileExt;
use itertools::Itertools;
use k8s_openapi::Metadata;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use log::info;
use serde::{Deserialize, Deserializer, Serialize};
use crate::skate::{exec_cmd, SupportedResources};

pub const CHECKBOX_EMOJI: char = '✔';
pub const CROSS_EMOJI: char = '✖';
pub const EQUAL_EMOJI: char = '~';
pub const INFO_EMOJI: &str = "[i]";
pub const TARGET: &str = include_str!(concat!(env!("OUT_DIR"), "/../output"));

pub fn slugify<S: AsRef<str>>(s: S) -> String {
    _slugify(s.as_ref())
}

#[doc(hidden)]
#[cfg(target_family = "wasm")]
#[wasm_bindgen(js_name = slugify)]
pub fn slugify_owned(s: String) -> String {
    _slugify(s.as_ref())
}

// avoid unnecessary monomorphizations
fn _slugify(s: &str) -> String {
    let mut slug: Vec<u8> = Vec::with_capacity(s.len());
    // Starts with true to avoid leading -
    let mut prev_is_dash = true;
    {
        let mut push_char = |x: u8| {
            match x {
                b'a'..=b'z' | b'0'..=b'9' => {
                    prev_is_dash = false;
                    slug.push(x);
                }
                b'A'..=b'Z' => {
                    prev_is_dash = false;
                    // Manual lowercasing as Rust to_lowercase() is unicode
                    // aware and therefore much slower
                    slug.push(x - b'A' + b'a');
                }
                _ => {
                    if !prev_is_dash {
                        slug.push(b'-');
                        prev_is_dash = true;
                    }
                }
            }
        };

        for c in s.chars() {
            if c.is_ascii() {
                (push_char)(c as u8);
            } else {
                for &cx in deunicode_char(c).unwrap_or("-").as_bytes() {
                    (push_char)(cx);
                }
            }
        }
    }

    // It's not really unsafe in practice, we know we have ASCII
    let mut string = unsafe { String::from_utf8_unchecked(slug) };
    if string.ends_with('-') {
        string.pop();
    }
    // We likely reserved more space than needed.
    string.shrink_to_fit();
    string
}

pub fn hash_string<T>(obj: T) -> String
where
    T: Hash,
{
    let mut hasher = DefaultHasher::new();
    obj.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}


// use with #[serde(deserialize_with = "deserialize_null_default")]
// null or nonexistant values will be deserialized as T::default(
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

pub fn calc_k8s_resource_hash(obj: (impl Metadata<Ty=ObjectMeta> + Serialize + Clone)) -> String
{
    let mut obj = obj.clone();

    let mut labels = obj.metadata().labels.clone().unwrap_or_default();
    labels.remove("skate.io/hash");
    labels = labels.into_iter().sorted_by_key(|l| l.1.clone()).map(|(k, v)| (k, v)).collect();
    obj.metadata_mut().labels = Option::from(labels);


    let mut annotations = obj.metadata().annotations.clone().unwrap_or_default();

    annotations = annotations.into_iter().sorted_by_key(|l| l.1.clone()).map(|(k, v)| (k, v)).collect();
    obj.metadata_mut().annotations = Option::from(annotations);

    let serialized = serde_yaml::to_string(&obj).unwrap();

    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}


#[derive(Serialize, Deserialize, Debug, Clone, Eq, Hash, PartialEq)]
pub struct NamespacedName {
    pub name: String,
    pub namespace: String,
}

impl From<&str> for NamespacedName {
    fn from(s: &str) -> Self {
        let parts: Vec<_> = s.split('.').collect();
        return Self {
            name: parts.first().unwrap_or(&"").to_string(),
            namespace: parts.last().unwrap_or(&"").to_string(),
        };
    }
}

impl Display for NamespacedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{}.{}", self.name, self.namespace).as_str())
    }
}

impl NamespacedName {
    pub fn new(name: String, namespace: String) -> Self {
        NamespacedName { name, namespace }
    }
}

// returns name, namespace
pub fn metadata_name(obj: &impl Metadata<Ty=ObjectMeta>) -> NamespacedName
{
    let m = obj.metadata();

    let name = m.labels.as_ref().and_then(|l| l.get("skate.io/name"));
    let ns = m.labels.as_ref().and_then(|l| l.get("skate.io/namespace"));

    if name.is_none() {
        panic!("metadata missing skate.io/name label")
    }

    if ns.is_none() {
        panic!("metadata missing skate.io/namespace label")
    }

    NamespacedName::new(name.unwrap().clone(), ns.unwrap().clone())
}

// hash_k8s_resource hashes a k8s resource and adds the hash to the labels, also returning it
pub fn hash_k8s_resource(obj: &mut (impl Metadata<Ty=ObjectMeta> + Serialize + Clone)) -> String

{
    let hash = calc_k8s_resource_hash(obj.clone());

    let mut labels = obj.metadata().labels.clone().unwrap_or_default();
    labels.insert("skate.io/hash".to_string(), hash.clone());
    obj.metadata_mut().labels = Option::from(labels);
    hash
}

// age returns the age of a resource in a human-readable format, with only the first segment of resolution (eg 2d1h4m  becomes 2d)
pub fn age(date_time: DateTime<Local>) -> String {
    match Local::now().signed_duration_since(date_time).to_std() {
        Ok(duration) => {
            if duration.as_secs() < 60 {
                return format!("{}s", duration.as_secs());
            }
            let minutes = duration.as_secs() / 60;
            if minutes < 60 {
                return format!("{}m", minutes);
            }
            let hours = duration.as_secs() / (60 * 60);
            if hours < 24 {
                return format!("{}h", hours);
            }

            let days = duration.as_secs() / (60 * 60 * 24);
            format!("{}d", days)
        }
        Err(_) => "".to_string()
    }
}

pub fn spawn_orphan_process<I, S>(cmd: &str, args: I)
where
    I: IntoIterator<Item=S>,
    S: AsRef<OsStr>,
{

    // The fact that we don't have a `?` or `unrwap` here is intentional
    // This disowns the process, which is what we want.
    let _ = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
pub fn lock_file<T>(file: &str, cb: Box<dyn FnOnce() -> Result<T, Box<dyn Error>>>) -> Result<T, Box<dyn Error>> {
    let lock_path = Path::new(file);
    let lock_file = File::create(lock_path).map_err(|e| anyhow!("failed to create/open lock file: {}", e))?;
    info!("waiting for lock on {}", lock_path.display());
    lock_file.lock_exclusive()?;
    info!("locked {}", lock_path.display());
    let result = cb();
    lock_file.unlock()?;
    info!("unlocked {}", lock_path.display());
    result
}

fn write_manifest_to_file(manifest: &str) -> Result<String, Box<dyn Error>> {
    let file_path = format!("/tmp/skate-{}.yaml", hash_string(manifest));
    let mut file = File::create(file_path.clone()).expect("failed to open file for manifests");
    file.write_all(manifest.as_ref()).expect("failed to write manifest to file");
    Ok(file_path)
}


pub fn apply_play(object: SupportedResources) -> Result<(), Box<dyn Error>> {
    let file_path = write_manifest_to_file(&serde_yaml::to_string(&object)?)?;

    let mut args = vec!["play", "kube", &file_path, "--start"];
    if !object.host_network() {
        args.push("--network=skate")
    }

    let result = exec_cmd("podman", &args)?;

    if !result.is_empty() {
        println!("{}", result);
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use chrono::{Duration, Local};
    use crate::util::age;

    #[test]
    fn test_age() {
        let conditions = &[
            (Local::now(), "0s"),
            (Local::now() - Duration::seconds(20), "20s"),
            (Local::now() - Duration::minutes(20), "20m"),
            (Local::now() - Duration::minutes(20*60), "20h"),
            (Local::now() - Duration::minutes(20*60*24), "20d"),
        ];

        for (input, expect) in conditions {
            let output = age(input.clone());
            assert_eq!(output, *expect, "input: {}", input);
        }
    }
}