//EVERYTHING HERE IS EXPERIMENTAL, FOR CREATING CANONICAL FILE IPFS CID
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use futures::StreamExt;
    use libipld::ipld;
    use rust_ipfs::UninitializedIpfsNoop as UninitializedIpfs;
    use rust_ipfs::{unixfs::UnixfsStatus, Ipfs, IpfsPath};

    use sha2::Digest;

    use crate::download_client::read_file_contents;

    #[tokio::test]
    async fn test_single_node() {
        // Initialize the repo and start a daemon
        let ipfs: Ipfs = UninitializedIpfs::new()
            .with_default()
            .add_listening_addr("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .with_mdns()
            .with_relay(true)
            .default_record_key_validator()
            .start()
            .await
            .unwrap();

        ipfs.default_bootstrap().await.unwrap();

        ipfs.bootstrap().await.unwrap();

        // Create a DAG
        let data = "0.0.0.0:5666";
        let cid = ipfs.put_dag(ipld!(data)).await.unwrap();
        let v0 = cid::multibase::Base::Base58Btc.encode(cid.hash().to_bytes());
        println!("the other publication with ipfs hyper client generated QmHash: Qmc7KRBmyUEtmPydjKA6SLAraz5RYyxjCQ6Zcam1CzoAaC; different from the current one. Must investigate why");
        println!("v1: {:?}\n from the ipfs client, we should get bafybeigmtfanhdyyevz3hbgnqk635rnldj5u3wq2vnbw6rgurm4se6u4ne", cid);
        assert_eq!(&v0, "QmQCJsNbR41bGj3uY1ErLX5HiFY9hXJQFsc81hmWtp4iQg");
        // Query DAG
        let ipfs_path = IpfsPath::from(cid);
        let block = ipfs.get_dag(ipfs_path).await.unwrap();
        assert_eq!(block, libipld::Ipld::String(data.to_string()));
        ipfs.exit_daemon().await;
    }

    #[tokio::test]
    async fn test_dag_creation() {
        // Initialize the repo and start a daemon
        let ipfs: Ipfs = UninitializedIpfs::new().start().await.unwrap();

        // Create a DAG
        let data = read_file_contents("../example-file/gravatar.sql")
            .await
            .expect("Read file");
        let cid1 = ipfs.put_dag(ipld!(data.clone())).await.unwrap();
        // let cid2 = ipfs.put_dag(ipld!("block2")).await?;
        // let root = ipld!([cid1, cid2]);
        // let cid = ipfs.put_dag(root).await?;
        // let path = IpfsPath::from(cid);
        println!("cid1: {:?}", cid1);
        // println!("v0: {:?}", cid1.to_string_of_base(cid::multibase::Base::Base58Btc));
        let v0 = cid::multibase::Base::Base58Btc.encode(cid1.hash().to_bytes());
        println!("v0: {:?}", v0);

        let ipfs_path = IpfsPath::from(cid1);
        // let path = ipfs.publish_ipns(&ipfs_path).await.unwrap();

        // println!("{ipfs_path} been published to {path}");

        // Query the DAG
        // let path1 = path.sub_path("0").unwrap();
        // let path2 = path.sub_path("1")?;
        let block = ipfs.get_dag(ipfs_path).await.unwrap();
        println!("block: {:?}", block);
        // let block2 = ipfs.get_dag(path2).await?;
        // println!("Received block with contents: {:?}", block1);
        // println!("Received block with contents: {:?}", block2);
        assert_eq!(block, libipld::Ipld::Bytes(data.clone()));
        // Exit
        ipfs.exit_daemon().await;
    }

    #[tokio::test]
    async fn test_unixfs_add() {
        tracing_subscriber::fmt::init();

        let ipfs: Ipfs = UninitializedIpfs::new()
            .with_default()
            .add_listening_addr("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .with_mdns()
            .start()
            .await
            .unwrap();

        let mut stream = ipfs.add_unixfs(PathBuf::from("../example-file/gravatar.sql"));

        while let Some(status) = stream.next().await {
            match status {
                UnixfsStatus::ProgressStatus {
                    written,
                    total_size,
                } => match total_size {
                    Some(size) => println!("{written} out of {size} stored"),
                    None => println!("{written} been stored"),
                },
                UnixfsStatus::FailedStatus {
                    written,
                    total_size,
                    error,
                } => {
                    match total_size {
                        Some(size) => println!("failed with {written} out of {size} stored"),
                        None => println!("failed with {written} stored"),
                    }

                    if let Some(error) = error {
                        panic!("{}", error);
                    } else {
                        panic!("Unknown error while writting to blockstore");
                    }
                }
                UnixfsStatus::CompletedStatus { path, written, .. } => {
                    println!("{written} been stored with path {path}");
                }
            }
        }
        ipfs.exit_daemon().await;
    }
}
