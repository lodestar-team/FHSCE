#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use std::{process::Command, time::Duration};
    use tempfile::tempdir;
    use tokio::fs;

    use file_exchange::{
        config::{DownloaderArgs, LocalDirectory},
        download_client::Downloader,
        manifest::ipfs::IpfsClient,
        test_util::server_ready,
    };

    #[tokio::test]
    async fn test_admin_publish_and_serve() {
        std::env::set_var("RUST_LOG", "off,file_exchange=trace,file_transfer=trace,file_service=trace,indexer_service=trace,indexer_common=trace");
        file_exchange::config::init_tracing("pretty").unwrap();

        let client = IpfsClient::new("https://ipfs.network.thegraph.com")
            .expect("Could not create client to thegraph IPFS gateway");
        // 1. Setup server
        let mut server_process = Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg("file-service")
            .arg("--")
            .arg("--config")
            .arg("./tests/test0.toml")
            .spawn()
            .expect("Failed to start server");
        tracing::debug!("Wait 10 seconds");
        tokio::time::sleep(Duration::from_secs(3)).await;
        let _ = server_ready("http://localhost:5677").await;

        // 2. admin command
        // Read status
        let bundles_query = json!({
            "query": r#"query {
              bundles{
                ipfsHash
              }
            }"#
        });
        let res = reqwest::Client::new()
            .post("http://0.0.0.0:5664/admin")
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer slayyy")
            .json(&bundles_query)
            .send()
            .await
            .expect("GraphQL admin bundle request gets response");
        assert!(res.status().is_success());
        let response_body: Value = res.json().await.expect("Status response is a json value");
        // Navigate through the JSON to extract the `ipfsHash` values
        let ipfs_hashes: Vec<String> = response_body["data"]["bundles"]
            .as_array()
            .expect("Bundles in response")
            .iter()
            .filter_map(|bundle| bundle["ipfsHash"].as_str().map(String::from))
            .collect();
        assert!(ipfs_hashes.len() == 1);

        // Mutation: publish and serve new files
        let graphql_query = json!({
            "query": r#"mutation {
                publishAndServeBundle(filenames: ["0017686116-5331aab87811944d-f8d105f60fa2e78d-17686021-default.dbin", "0017686115-f8d105f60fa2e78d-7d23a3e458beaff1-17686021-default.dbin"]) {
                    ipfsHash
                }
            }"#
        });

        let res = reqwest::Client::new()
            .post("http://0.0.0.0:5664/admin")
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer slayyy")
            .json(&graphql_query)
            .send()
            .await
            .expect("GraphQL admin mutation request gets a response");

        assert!(res.status().is_success());
        let response_body: Value = res.json().await.expect("Response is a json value");
        let ipfs_hash = response_body["data"]["publishAndServeBundle"]["ipfsHash"]
            .as_str()
            .expect("Response has IPFS hash");
        assert!(!ipfs_hashes.contains(&ipfs_hash.to_string())); // should be a new hash

        // Check public status (checking 5665/admin should be the same)
        let res = reqwest::Client::new()
            .post("http://0.0.0.0:5677/files-status")
            .header("Content-Type", "application/json")
            .json(&bundles_query)
            .send()
            .await
            .expect("GraphQL status bundle request gets response");
        assert!(res.status().is_success());
        let response_body: Value = res.json().await.expect("Status response is a json value");
        // Navigate through the JSON to extract the `ipfsHash` values
        let ipfs_hashes: Vec<String> = response_body["data"]["bundles"]
            .as_array()
            .expect("Bundles in response")
            .iter()
            .filter_map(|bundle| bundle["ipfsHash"].as_str().map(String::from))
            .collect();
        assert!(ipfs_hashes.contains(&ipfs_hash.to_string())); // should be a new hash

        // 3. Setup downloader
        let temp_dir = tempdir().unwrap();
        let main_dir = temp_dir.path().to_path_buf();

        let downloader_args = DownloaderArgs {
            storage_method: file_exchange::config::StorageMethod::LocalFiles(LocalDirectory {
                main_dir: main_dir.to_str().unwrap().to_string(),
            }),
            ipfs_hash: ipfs_hash.to_string(),
            indexer_endpoints: [
                "http://localhost:5679".to_string(),
                "http://localhost:5677".to_string(),
            ]
            .to_vec(),
            verifier: None,
            mnemonic: None,
            free_query_auth_token: Some("Bearer free-token".to_string()),
            provider: None,
            provider_concurrency: 2,
            ..Default::default()
        };

        let downloader = Downloader::new(client, downloader_args).await;

        // 4. Perform the download
        let download_result = downloader.download_target().await;

        // 5. Validate the download
        tracing::info!(
            result = tracing::field::debug(&download_result),
            "Download result"
        );
        assert!(download_result.is_ok());
        // Further checks can be added to verify the contents of the downloaded files

        // 6. Cleanup
        fs::remove_dir_all(temp_dir).await.unwrap();
        let _ = server_process.kill();
    }
}
