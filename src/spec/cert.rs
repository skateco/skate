use k8s_openapi::{apimachinery, ClusterResourceScope, Metadata, Resource};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClusterIssuer {
    /// Standard object's metadata. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#metadata
    pub metadata: apimachinery::pkg::apis::meta::v1::ObjectMeta,

    /// spec is the desired state of the Ingress. More info: https://git.k8s.io/community/contributors/devel/sig-architecture/api-conventions.md#spec-and-status
    pub spec: Option<ClusterIssuerSpec>,
}

impl Resource for ClusterIssuer {
    const API_VERSION: &'static str = "cert-manager.io/v1";
    const GROUP: &'static str = "";
    const KIND: &'static str = "ClusterIssuer";
    const VERSION: &'static str = "";
    const URL_PATH_SEGMENT: &'static str = "clusterissuer";
    type Scope = ClusterResourceScope;
}

impl Metadata for ClusterIssuer {
    type Ty = apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn metadata(&self) -> &<Self as Metadata>::Ty {
        &self.metadata
    }

    fn metadata_mut(&mut self) -> &mut <Self as Metadata>::Ty {
        &mut self.metadata
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct ClusterIssuerSpec {
    pub acme: Acme,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Acme {
    pub email: String,
    pub server: String,
}

impl<'de> serde::Deserialize<'de> for ClusterIssuer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[allow(non_camel_case_types)]
        enum Field {
            Key_api_version,
            Key_kind,
            Key_metadata,
            Key_spec,
            Other,
        }

        impl<'de> serde::Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct Visitor;

                impl<'de> serde::de::Visitor<'de> for Visitor {
                    type Value = Field;

                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("field identifier")
                    }

                    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                    where
                        E: serde::de::Error,
                    {
                        Ok(match v {
                            "apiVersion" => Field::Key_api_version,
                            "kind" => Field::Key_kind,
                            "metadata" => Field::Key_metadata,
                            "spec" => Field::Key_spec,
                            _ => Field::Other,
                        })
                    }
                }

                deserializer.deserialize_identifier(Visitor)
            }
        }

        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ClusterIssuer;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(<Self::Value as Resource>::KIND)
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut value_metadata: Option<apimachinery::pkg::apis::meta::v1::ObjectMeta> =
                    None;
                let mut value_spec: Option<ClusterIssuerSpec> = None;

                while let Some(key) = serde::de::MapAccess::next_key::<Field>(&mut map)? {
                    match key {
                        Field::Key_api_version => {
                            let value_api_version: String =
                                serde::de::MapAccess::next_value(&mut map)?;
                            if value_api_version != <Self::Value as Resource>::API_VERSION {
                                return Err(serde::de::Error::invalid_value(
                                    serde::de::Unexpected::Str(&value_api_version),
                                    &<Self::Value as Resource>::API_VERSION,
                                ));
                            }
                        }
                        Field::Key_kind => {
                            let value_kind: String = serde::de::MapAccess::next_value(&mut map)?;
                            if value_kind != <Self::Value as Resource>::KIND {
                                return Err(serde::de::Error::invalid_value(
                                    serde::de::Unexpected::Str(&value_kind),
                                    &<Self::Value as Resource>::KIND,
                                ));
                            }
                        }
                        Field::Key_metadata => {
                            value_metadata = serde::de::MapAccess::next_value(&mut map)?
                        }
                        Field::Key_spec => value_spec = serde::de::MapAccess::next_value(&mut map)?,
                        Field::Other => {
                            let _: serde::de::IgnoredAny =
                                serde::de::MapAccess::next_value(&mut map)?;
                        }
                    }
                }

                Ok(ClusterIssuer {
                    metadata: value_metadata.unwrap_or_default(),
                    spec: value_spec,
                })
            }
        }

        deserializer.deserialize_struct(
            <Self as Resource>::KIND,
            &["apiVersion", "kind", "metadata", "spec"],
            Visitor,
        )
    }
}

impl serde::Serialize for ClusterIssuer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct(
            <Self as Resource>::KIND,
            3 + self.spec.as_ref().map_or(0, |_| 1),
        )?;
        serde::ser::SerializeStruct::serialize_field(
            &mut state,
            "apiVersion",
            <Self as Resource>::API_VERSION,
        )?;
        serde::ser::SerializeStruct::serialize_field(&mut state, "kind", <Self as Resource>::KIND)?;
        serde::ser::SerializeStruct::serialize_field(&mut state, "metadata", &self.metadata)?;
        if let Some(value) = &self.spec {
            serde::ser::SerializeStruct::serialize_field(&mut state, "spec", value)?;
        }
        serde::ser::SerializeStruct::end(state)
    }
}
