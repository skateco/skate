use std::error::Error;
use async_ssh2_tokio::client::{Client};
use strum_macros::EnumString;

pub struct SshClient {
    pub client: Client,
}

#[derive(Debug, EnumString)]
pub enum Os {
    Unknown,
    Linux,
    Darwin,
}

#[derive(Debug)]
pub struct Platform {
    pub arch: String,
    pub os: Os,
    pub distribution: Option<String>,
}

#[derive(Debug)]
pub struct HostInfo {
    pub hostname: String,
    pub platform: Platform,
    pub skatelet_version: Option<String>,
}

impl SshClient {
    pub async fn get_host_info(self) -> Result<HostInfo, Box<dyn Error>> {
        // returns for example:
        // ras-pi
        // armv7l
        // Linux
        // Raspbian
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
        let distro = lines.next().map(String::from).filter(|s| !s.is_empty());
        let skatelet_version = lines.next().map(String::from).filter(|s| !s.is_empty());;

        return Ok(HostInfo {
            hostname,
            platform: Platform {
                arch,
                os: Os::Unknown,
                distribution: distro,
            },
            skatelet_version,
        });
    }
}