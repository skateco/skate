use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use deunicode::deunicode_char;
use itertools::Itertools;
use k8s_openapi::{Metadata, NamespaceResourceScope, Resource};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde::{Deserialize, Deserializer, Serialize};

pub const CHECKBOX_EMOJI: char = '✅';
pub const CROSS_EMOJI: char = '❌';
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

pub fn calc_k8s_resource_hash(obj: (impl Metadata<Scope=NamespaceResourceScope, Ty=ObjectMeta> + Serialize + Clone)) -> String
{
    let mut obj = obj.clone();

    let mut labels = obj.metadata().labels.clone().unwrap_or_default();
    labels.remove("skate.io/hash");
    labels = labels.into_iter().sorted_by_key(|l| l.1.clone()).map(|(k, v)| (k, v)).collect();

    let mut annotations = obj.metadata().annotations.clone().unwrap_or_default();

    annotations = annotations.into_iter().sorted_by_key(|l| l.1.clone()).map(|(k, v)| (k, v)).collect();
    obj.metadata_mut().annotations = Option::from(annotations);

    let serialized = serde_yaml::to_string(&obj).unwrap();

    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub fn hash_k8s_resource(obj: &mut (impl Metadata<Scope=NamespaceResourceScope, Ty=ObjectMeta> + Serialize + Clone)) -> String

{
    let hash = calc_k8s_resource_hash(obj.clone());

    let mut labels = obj.metadata().labels.clone().unwrap_or_default();
    labels.insert("skate.io/hash".to_string(), hash.clone());
    obj.metadata_mut().labels = Option::from(labels);
    hash
}