use anyhow::anyhow;
use std::fs::read;

pub fn get_resolv_conf_dns() -> anyhow::Result<String> {
    let resolver = read("/etc/resolv.conf")?;
    for line in String::from_utf8_lossy(&resolver).lines() {
        if line.starts_with("nameserver") {
            let parts = line.split_ascii_whitespace().collect::<Vec<_>>();
            return Ok(parts[1].to_string());
        }
    }
    Err(anyhow!("failed to find dns line in /etc/resolv.conf"))
}
