use std::error::Error;
use anyhow::anyhow;
use semver::Version;
use serde::{Deserialize, Serialize};
use crate::skate::Platform;
// Name your user agent after your app?
static APP_USER_AGENT: &str = concat!(
env!("CARGO_PKG_NAME"),
"/",
env!("CARGO_PKG_VERSION"),
);

pub struct Client {
    reqwest_client: reqwest::Client,
}
impl Client {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .build().unwrap();
        Client {
            reqwest_client: client
        }
    }
    pub async fn get_latest_release(&self) -> Result<Release, reqwest::Error> {


        // From the documentation (https://docs.github.com/en/rest/releases/releases?apiVersion=2022-11-28)
        // "The latest release is the most recent non-prerelease, non-draft release, sorted by the created_at attribute."
        self.reqwest_client.get("https://api.github.com/repos/skateco/skate/releases/latest")
            .send()
            .await?
            .json::<Release>()
            .await
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Asset {
    pub url: Option<String>,
    pub name: Option<String>,
    pub label: Option<String>,
    pub content_type: Option<String>,
    pub state: Option<String>,
    pub browser_download_url: Option<String>,
}


#[derive(Debug, Serialize, Deserialize)]
pub struct Release {
    pub name: Option<String>,
    pub tag_name: Option<String>,
    pub prerelease: Option<bool>,
    pub created_at: Option<String>,
    pub published_at: Option<String>,
    pub assets: Option<Vec<Asset>>,
}

impl Release {
    pub fn version(&self) -> Result<Version, Box<dyn Error>> {
        if self.tag_name.is_none() {
            return Err(anyhow!("missing tag_name field in response").into());
        }
        let result = Version::parse(self.tag_name.as_ref().unwrap().as_str().strip_prefix("v").unwrap_or(""))?;
        Ok(result)
        
    }
    pub fn find_skatelet_archive(&self, platform: &Platform) -> Option<String> {
        if self.assets.is_none() {
            return None;
        }
        let asset = self.assets.as_ref().unwrap().iter().find(|asset| {
            let (dl_arch, dl_gnu) = match platform.arch.as_str() {
                "amd64" => ("x86_64", "gnu"),
                "armv6l" => ("arm", "gnueabi"),
                "armv7l" => ("arm7", "gnueabi"),
                "arm64" => ("aarch64", "gnu"),
                _ => (platform.arch.as_str(), "gnu")
            };

            let filename = format!("skatelet-{}-unknown-linux-{}.tar.gz", dl_arch, dl_gnu);
            if asset.name.is_none() {
                return false;
            }

            if asset.name.as_ref().unwrap() != &filename {
                return false;
            }

            if asset.browser_download_url.is_none() {
                return false;
            }
            if asset.browser_download_url.as_ref().unwrap().is_empty() {
                return false;
            }

            return true;
        });

        if asset.is_none() {
            return None;
        }

        Some(asset.unwrap().browser_download_url.clone().unwrap())
    }
}

#[cfg(test)]
mod tests {
    // use crate::github::Client;
    //
    //
    // #[tokio::test]
    // async fn test_get_release() {
    //     let client = Client::new();
    //     let release = client.get_latest_release().await;
    //
    //     assert!(release.is_ok(), "{:?}", release.err());
    //     let release = release.unwrap();
    //
    //
    //     println!("{:?}", release);
    //     let version = release.version();
    //     assert!(version.is_ok(), "{:?}", version.err());
    // }
}