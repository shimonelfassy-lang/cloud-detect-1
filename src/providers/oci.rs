//! Oracle Cloud Infrastructure (OCI).

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::mpsc::Sender;
use tracing::{debug, error, instrument};

use crate::{Provider, ProviderId};

const METADATA_URI: &str = "http://169.254.169.254";
const METADATA_PATH_V1: &str = "/opc/v1/instance/";
const METADATA_PATH_V2: &str = "/opc/v2/instance/";
const VENDOR_FILE: &str = "/sys/class/dmi/id/chassis_asset_tag";
pub(crate) const IDENTIFIER: ProviderId = ProviderId::OCI;

#[derive(Serialize, Deserialize)]
struct MetadataResponse {
    id: String,
}

pub(crate) struct Oci;

#[async_trait]
impl Provider for Oci {
    fn identifier(&self) -> ProviderId {
        IDENTIFIER
    }

    /// Tries to identify OCI using all the implemented options.
    #[instrument(skip_all)]
    async fn identify(&self, tx: Sender<ProviderId>, timeout: Duration) {
        debug!("Checking Oracle Cloud Infrastructure");
        if self.check_vendor_file(VENDOR_FILE).await
            || self
                .check_metadata_server_imdsv2(METADATA_URI, timeout)
                .await
            || self
                .check_metadata_server_imdsv1(METADATA_URI, timeout)
                .await
        {
            debug!("Identified Oracle Cloud Infrastructure");
            let res = tx.send(IDENTIFIER).await;

            if let Err(err) = res {
                error!("Error sending message: {:?}", err);
            }
        }
    }
}

impl Oci {
    /// Tries to identify OCI via metadata server (using IMDSv2).
    #[instrument(skip_all)]
    async fn check_metadata_server_imdsv2(&self, metadata_uri: &str, timeout: Duration) -> bool {
        let url = format!("{metadata_uri}{METADATA_PATH_V2}");
        debug!("Checking {} metadata using url: {}", IDENTIFIER, url);

        let client = if let Ok(client) = reqwest::Client::builder().timeout(timeout).build() {
            client
        } else {
            error!("Error creating client");
            return false;
        };

        match client
            .get(url)
            .header("Authorization", "Bearer Oracle")
            .send()
            .await
        {
            Ok(resp) => match resp.json::<MetadataResponse>().await {
                Ok(resp) => resp.id.starts_with("ocid1.instance."),
                Err(err) => {
                    debug!("Error reading response: {:?}", err);
                    false
                }
            },
            Err(err) => {
                debug!("Error making request: {:?}", err);
                false
            }
        }
    }

    /// Tries to identify OCI via metadata server (using IMDSv1).
    #[instrument(skip_all)]
    async fn check_metadata_server_imdsv1(&self, metadata_uri: &str, timeout: Duration) -> bool {
        let url = format!("{metadata_uri}{METADATA_PATH_V1}");
        debug!("Checking {} metadata using url: {}", IDENTIFIER, url);

        let client = if let Ok(client) = reqwest::Client::builder().timeout(timeout).build() {
            client
        } else {
            error!("Error creating client");
            return false;
        };

        match client.get(url).send().await {
            Ok(resp) => match resp.json::<MetadataResponse>().await {
                Ok(resp) => resp.id.starts_with("ocid1.instance."),
                Err(err) => {
                    debug!("Error reading response: {:?}", err);
                    false
                }
            },
            Err(err) => {
                debug!("Error making request: {:?}", err);
                false
            }
        }
    }

    /// Tries to identify OCI using vendor file(s).
    #[instrument(skip_all)]
    async fn check_vendor_file<P: AsRef<Path>>(&self, vendor_file: P) -> bool {
        debug!(
            "Checking {} vendor file: {}",
            IDENTIFIER,
            vendor_file.as_ref().display()
        );

        if vendor_file.as_ref().is_file() {
            return match fs::read_to_string(vendor_file).await {
                Ok(content) => content.contains("OracleCloud"),
                Err(err) => {
                    debug!("Error reading file: {:?}", err);
                    false
                }
            };
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use anyhow::Result;
    use tempfile::NamedTempFile;
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[tokio::test]
    async fn test_check_metadata_server_imdsv2_success() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH_V2))
            .and(header("Authorization", "Bearer Oracle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                id: "ocid1.instance.oc1.phx.123abc".to_string(),
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Oci;
        let metadata_uri = mock_server.uri();
        let result = provider
            .check_metadata_server_imdsv2(&metadata_uri, Duration::from_secs(1))
            .await;

        assert!(result);
    }

    #[tokio::test]
    async fn test_check_metadata_server_imdsv2_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH_V2))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                id: "i-notoracle".to_string(),
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Oci;
        let metadata_uri = mock_server.uri();
        let result = provider
            .check_metadata_server_imdsv2(&metadata_uri, Duration::from_secs(1))
            .await;

        assert!(!result);
    }

    #[tokio::test]
    async fn test_check_metadata_server_imdsv1_success() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH_V1))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                id: "ocid1.instance.oc1.phx.123abc".to_string(),
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Oci;
        let metadata_uri = mock_server.uri();
        let result = provider
            .check_metadata_server_imdsv1(&metadata_uri, Duration::from_secs(1))
            .await;

        assert!(result);
    }

    #[tokio::test]
    async fn test_check_metadata_server_imdsv1_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(path(METADATA_PATH_V1))
            .respond_with(ResponseTemplate::new(200).set_body_json(MetadataResponse {
                id: "i-notoracle".to_string(),
            }))
            .expect(1)
            .mount(&mock_server)
            .await;

        let provider = Oci;
        let metadata_uri = mock_server.uri();
        let result = provider
            .check_metadata_server_imdsv1(&metadata_uri, Duration::from_secs(1))
            .await;

        assert!(!result);
    }

    #[tokio::test]
    async fn test_check_vendor_file_success() -> Result<()> {
        let mut vendor_file = NamedTempFile::new()?;
        vendor_file.write_all(b"OracleCloud")?;

        let provider = Oci;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(result);

        Ok(())
    }

    #[tokio::test]
    async fn test_check_vendor_file_failure() -> Result<()> {
        let vendor_file = NamedTempFile::new()?;

        let provider = Oci;
        let result = provider.check_vendor_file(vendor_file.path()).await;

        assert!(!result);

        Ok(())
    }
}
