use crate::config::{BundleArgs, PublisherArgs};
use crate::errors::Error;
use crate::manifest::store::Store;
use crate::manifest::{
    ipfs::{AddResponse, IpfsClient},
    BlockRange, BundleManifest, FileMetaInfo,
};
use object_store::path::Path;
use object_store::ObjectMeta;
use serde_yaml::to_string;

pub struct ManifestPublisher {
    ipfs_client: IpfsClient,
    store: Store,
    config: PublisherArgs,
}

impl ManifestPublisher {
    pub fn new(ipfs_client: IpfsClient, config: PublisherArgs) -> Self {
        let store = Store::new(&config.storage_method).expect("Create store");

        ManifestPublisher {
            ipfs_client,
            store,
            config,
        }
    }

    /// Takes file_path, create file_manifest, build merkle tree, publish, write to output
    pub async fn hash_and_publish_file(
        &self,
        object_meta: &ObjectMeta,
    ) -> Result<AddResponse, Error> {
        let yaml_str = self.write_file_manifest(object_meta).await?;

        let added: AddResponse = self
            .ipfs_client
            .add(yaml_str.as_bytes().to_vec())
            .await
            .map_err(Error::IPFSError)?;
        tracing::debug!(
            added = tracing::field::debug(&added),
            "Added yaml file to IPFS"
        );

        Ok(added)
    }

    pub async fn hash_and_publish_files(&self) -> Result<Vec<FileMetaInfo>, Error> {
        let mut root_hashes: Vec<FileMetaInfo> = Vec::new();
        let mut to_publish: Vec<ObjectMeta> = Vec::new();
        // grab all files
        for prefix in &self.config.prefixes {
            let files = self
                .store
                .list(Some(&Path::from(prefix.to_string())))
                .await
                .unwrap_or_default();
            to_publish = union_obj_metas(to_publish, files);
        }
        for filename in &self.config.filenames {
            let object_meta =
                self.store
                    .find_object(filename, None)
                    .await
                    .ok_or(Error::DataUnavailable(format!(
                        "Did not find object {:?}",
                        filename,
                    )))?;
            if !to_publish.contains(&object_meta) {
                to_publish.push(object_meta);
            }
        }

        tracing::trace!(
            to_publish = tracing::field::debug(&to_publish),
            "Publish these files/objects",
        );

        for object_meta in to_publish {
            let ipfs_hash = self.hash_and_publish_file(&object_meta).await?.hash;
            root_hashes.push(FileMetaInfo {
                name: object_meta
                    .location
                    .filename()
                    .unwrap_or_default()
                    .to_string(),
                hash: ipfs_hash,
            });
        }

        Ok(root_hashes)
    }

    pub fn construct_bundle_manifest(
        &self,
        bundle_args: &BundleArgs,
        file_meta_info: Vec<FileMetaInfo>,
    ) -> Result<String, Error> {
        let manifest = BundleManifest {
            files: file_meta_info,
            file_type: bundle_args.file_type.clone(),
            spec_version: bundle_args.bundle_version.clone(),
            description: bundle_args.description.clone(),
            chain_id: bundle_args.chain_id.clone(),
            block_range: BlockRange {
                start_block: bundle_args.start_block,
                end_block: bundle_args.end_block,
            },
        };
        let yaml = serde_yaml::to_string(&manifest).map_err(Error::YamlError)?;
        Ok(yaml)
    }

    pub async fn publish_bundle_manifest(&self, manifest_yaml: &str) -> Result<String, Error> {
        let ipfs_hash = self
            .ipfs_client
            .add(manifest_yaml.as_bytes().to_vec())
            .await
            .map_err(Error::IPFSError)?
            .hash;

        Ok(ipfs_hash)
    }

    pub async fn publish(&self) -> Result<Vec<String>, Error> {
        let meta_info = self.hash_and_publish_files().await?;

        tracing::trace!(
            meta_info = tracing::field::debug(&meta_info),
            "hash_and_publish_files",
        );

        // Files are published, now publish the bundle if specified
        if let Some(bundle_args) = &self.config.bundle {
            match self.construct_bundle_manifest(bundle_args, meta_info) {
                Ok(manifest_yaml) => {
                    let ipfs_hash = self.publish_bundle_manifest(&manifest_yaml).await?;
                    tracing::info!(
                        "Published bundle manifest to IPFS with hash: {}",
                        &ipfs_hash
                    );
                    Ok(vec![ipfs_hash])
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(meta_info
                .into_iter()
                .map(|m| m.hash.clone())
                .collect::<Vec<String>>())
        }
    }

    // publish by prefixes
    pub async fn write_file_manifest(&self, object_meta: &ObjectMeta) -> Result<String, Error> {
        let file_manifest = self
            .store
            .file_manifest(object_meta, Some(self.config.chunk_size as usize))
            .await?;

        tracing::trace!(
            file = tracing::field::debug(&file_manifest),
            object_meta = tracing::field::debug(&object_meta),
            "Created file manifest for the object"
        );

        let yaml = to_string(&file_manifest).map_err(Error::YamlError)?;
        Ok(yaml)
    }
}

fn union_obj_metas(vec1: Vec<ObjectMeta>, vec2: Vec<ObjectMeta>) -> Vec<ObjectMeta> {
    let mut result = vec![];
    fn contains(result: &[ObjectMeta], meta: &ObjectMeta) -> bool {
        result.iter().any(|item| item == meta)
    }

    for item in vec1.into_iter() {
        if !contains(&result, &item) {
            result.push(item);
        }
    }
    for item in vec2.into_iter() {
        if !contains(&result, &item) {
            result.push(item);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LocalDirectory, StorageMethod};

    #[tokio::test]
    async fn test_write_file_manifest() {
        let client = IpfsClient::localhost();
        let args = PublisherArgs {
            storage_method: StorageMethod::LocalFiles(LocalDirectory {
                main_dir: String::from("../example-file"),
            }),
            chunk_size: 1048576,
            ..Default::default()
        };
        let publisher = ManifestPublisher::new(client, args);
        let name = "example-create-17686085.dbin";

        // Hash and publish a single file
        let object_meta = publisher
            .store
            .find_object(name, None)
            .await
            .expect("find object");
        let file_manifest_yaml = publisher.write_file_manifest(&object_meta).await;

        assert!(file_manifest_yaml.is_ok());
    }

    #[tokio::test]
    #[ignore] // Run when there is a localhost IPFS node
    async fn test_publish() {
        let client = IpfsClient::localhost();
        let args = PublisherArgs {
            storage_method: StorageMethod::LocalFiles(LocalDirectory {
                main_dir: String::from("../example-file"),
            }),
            ..Default::default()
        };
        let publisher = ManifestPublisher::new(client, args);
        let name = "example-create-17686085.dbin";

        let object_meta = publisher
            .store
            .find_object(name, None)
            .await
            .expect("find object");
        // Hash and publish a single file
        let hash = publisher
            .hash_and_publish_file(&object_meta)
            .await
            .unwrap()
            .hash;

        // Construct and publish a bundle manifest
        let meta_info = vec![FileMetaInfo {
            name: name.to_string(),
            hash,
        }];

        if let Ok(manifest_yaml) =
            publisher.construct_bundle_manifest(&BundleArgs::default(), meta_info)
        {
            if let Ok(ipfs_hash) = publisher.publish_bundle_manifest(&manifest_yaml).await {
                tracing::info!("Published bundle manifest to IPFS with hash: {}", ipfs_hash);
            }
        }
    }
}
