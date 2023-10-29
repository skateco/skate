use std::error::Error;
use async_ssh2_tokio::client::Client;
use crate::skate::{Distribution, Os, Platform};

pub struct SshClient {
    pub client: Client,
}

#[derive(Debug, Clone)]
pub struct HostInfoResponse {
    pub hostname: String,
    pub platform: Platform,
    pub skatelet_version: Option<String>,
}

impl SshClient {
    pub async fn get_host_info(&self) -> Result<HostInfoResponse, Box<dyn Error>> {
        let command = "\
hostname=`hostname`;
arch=`arch`;
os=`uname -s`;
distro=`cat /etc/issue|head -1|awk '{print $1}'`;
skatelet_version=`skatelet --version`;

echo $hostname;
echo $arch;
echo $os;
echo $distro;
echo $skatelet_version;
";

        let result = self.client.execute(command).await.expect("ssh command failed");

        let mut lines = result.stdout.lines();

        let hostname = lines.next().expect("missing hostname").to_string();
        let arch = lines.next().expect("missing arch").to_string();
        lines.next();
        let distro = Distribution::from(lines.next().map(String::from).unwrap_or_default());
        let skatelet_version = lines.next().map(String::from).filter(|s| !s.is_empty());
        ;

        return Ok(HostInfoResponse {
            hostname,
            platform: Platform {
                arch,
                os: Os::Unknown,
                distribution: distro,
            },
            skatelet_version,
        });
    }

    pub async fn download_skatelet(&self, platform: Platform) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}