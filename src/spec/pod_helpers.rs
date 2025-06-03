use k8s_openapi::api::core::v1::PodSpec;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("invalid quantity for {0}: {1}")]
    InvalidQuantity(String, String),
}

pub struct ResourceRequests {
    pub cpu_millis: Option<u64>,
    pub memory_bytes: Option<u64>,
}

/// returns cpu in millis
pub fn parse_cpu_quantity(cpu: &Quantity) -> Result<u64, Error> {
    // if it's just a number, parse that and convert to millis
    if let Ok(cpu_millis) = cpu.0.parse::<f32>() {
        return Ok((cpu_millis * 1000.0).round() as u64);
    }

    if let Some(cpu_stripped) = cpu.0.strip_suffix("m") {
        if let Ok(cpu_millis) = cpu_stripped.parse::<u64>() {
            return Ok(cpu_millis);
        }
    }
    Err(Error::InvalidQuantity("cpu".to_string(), cpu.0.clone()))
}

/// returns memory in bytes
pub fn parse_memory_quantity(memory: &Quantity) -> Result<u64, Error> {
    // if it's just a number, parse that and convert to bytes
    if let Ok(memory_bytes) = memory.0.parse::<u64>() {
        return Ok(memory_bytes);
    }
    // handle memory suffixes like Ki, Mi, Gi, etc.
    if let Some(suffix) = memory.0.strip_suffix("Ki") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1024);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("Mi") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1024 * 1024);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("Gi") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1024 * 1024 * 1024);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("Ti") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1024 * 1024 * 1024 * 1024);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("Pi") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1024 * 1024 * 1024 * 1024 * 1024);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("Ei") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1024 * 1024 * 1024 * 1024 * 1024 * 1024);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("K") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1000);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("M") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1000 * 1000);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("G") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1000 * 1000 * 1000);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("T") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1000 * 1000 * 1000 * 1000);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("P") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1000 * 1000 * 1000 * 1000 * 1000);
        }
    } else if let Some(suffix) = memory.0.strip_suffix("E") {
        if let Ok(value) = suffix.parse::<u64>() {
            return Ok(value * 1000 * 1000 * 1000 * 1000 * 1000 * 1000);
        }
    }

    Err(Error::InvalidQuantity(
        "memory".to_string(),
        memory.0.clone(),
    ))
}

pub fn get_requests(p: &PodSpec) -> Result<ResourceRequests, Error> {
    let mut cpu_millis = 0;
    let mut memory_bytes = 0;

    for c in &p.containers {
        if let Some(resources) = &c.resources {
            if let Some(requests) = &resources.requests {
                if let Some(cpu) = requests.get("cpu") {
                    cpu_millis += parse_cpu_quantity(cpu)?;
                }
                if let Some(memory) = requests.get("memory") {
                    memory_bytes += parse_memory_quantity(memory)?;
                }
            }
        }
    }
    let mut max_init_cpu_millis = 0;
    let mut max_init_memory_bytes = 0;

    if let Some(init_containers) = &p.init_containers {
        for c in init_containers {
            if let Some(resources) = &c.resources {
                if let Some(requests) = &resources.requests {
                    if let Some(cpu) = requests.get("cpu") {
                        max_init_cpu_millis = max_init_cpu_millis.max(parse_cpu_quantity(cpu)?);
                    }
                    if let Some(memory) = requests.get("memory") {
                        max_init_memory_bytes =
                            max_init_memory_bytes.max(parse_memory_quantity(memory)?);
                    }
                }
            }
        }
    }

    // take max of init containers and regular containers
    cpu_millis = cpu_millis.max(max_init_cpu_millis);
    memory_bytes = memory_bytes.max(max_init_memory_bytes);

    Ok(ResourceRequests {
        cpu_millis: if cpu_millis > 0 {
            Some(cpu_millis)
        } else {
            None
        },
        memory_bytes: if memory_bytes > 0 {
            Some(memory_bytes)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn should_parse_cpu_quantity() -> Result<(), super::Error> {
        use super::*;
        assert_eq!(parse_cpu_quantity(&Quantity("200m".to_string()))?, 200);
        assert_eq!(parse_cpu_quantity(&Quantity("1".to_string()))?, 1000);
        assert_eq!(parse_cpu_quantity(&Quantity("500".to_string()))?, 500000);
        assert_eq!(parse_cpu_quantity(&Quantity("2".to_string()))?, 2000);

        let result = parse_cpu_quantity(&Quantity("invalid".to_string()));
        assert!(result.is_err());
        dbg!(&result);

        Ok(())
    }

    #[test]
    fn should_parse_memory_quantity() -> Result<(), super::Error> {
        use super::*;
        assert_eq!(
            parse_memory_quantity(&Quantity("200Mi".to_string()))?,
            200 * 1024 * 1024
        );
        assert_eq!(
            parse_memory_quantity(&Quantity("1Gi".to_string()))?,
            1024 * 1024 * 1024
        );
        assert_eq!(
            parse_memory_quantity(&Quantity("500Ki".to_string()))?,
            500 * 1024
        );
        assert_eq!(
            parse_memory_quantity(&Quantity("2Ti".to_string()))?,
            2 * 1024 * 1024 * 1024 * 1024
        );

        assert_eq!(
            parse_memory_quantity(&Quantity("200M".to_string()))?,
            200 * 1000 * 1000
        );
        assert_eq!(
            parse_memory_quantity(&Quantity("1G".to_string()))?,
            1000 * 1000 * 1000
        );
        assert_eq!(
            parse_memory_quantity(&Quantity("500K".to_string()))?,
            500 * 1000
        );
        assert_eq!(
            parse_memory_quantity(&Quantity("2T".to_string()))?,
            2 * 1000 * 1000 * 1000 * 1000
        );

        let result = parse_memory_quantity(&Quantity("invalid".to_string()));
        assert!(result.is_err());
        dbg!(&result);

        Ok(())
    }

    #[test]
    fn should_get_requests() -> Result<(), super::Error> {
        use super::*;
        let pod_spec = PodSpec {
            containers: vec![k8s_openapi::api::core::v1::Container {
                name: "test-container".to_string(),
                resources: Some(k8s_openapi::api::core::v1::ResourceRequirements {
                    requests: Some(std::collections::BTreeMap::from([
                        ("cpu".to_string(), Quantity("200m".to_string())),
                        ("memory".to_string(), Quantity("200Mi".to_string())),
                    ])),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };

        let requests = get_requests(&pod_spec)?;
        assert_eq!(requests.cpu_millis, Some(200));
        assert_eq!(requests.memory_bytes, Some(200 * 1024 * 1024));

        Ok(())
    }
}
