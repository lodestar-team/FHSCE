use async_graphql::SimpleObject;
use object_store::path::Path;

pub mod file_hasher;
pub mod file_reader;
pub mod ipfs;
pub mod manifest_fetcher;
pub mod store;

use serde::{Deserialize, Serialize};

use crate::{
    errors::Error,
    manifest::{file_hasher::verify_chunk, ipfs::is_valid_ipfs_hash},
};

/* Public Manifests */

/// Better mapping of files and file manifests
#[derive(Serialize, Deserialize, Clone, Debug, SimpleObject)]
pub struct BundleManifest {
    pub files: Vec<FileMetaInfo>,
    pub file_type: Option<String>,
    pub spec_version: Option<String>,
    pub description: Option<String>,
    pub chain_id: Option<String>,
    pub block_range: BlockRange,
    // pub identifier: String,
    // pub publisher_url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, SimpleObject)]
// #[graphql(input_name = "MyObjInput")] // Note: You must use the input_name attribute to define a new name for the input type, otherwise a runtime error will occur.
pub struct FileMetaInfo {
    pub name: String,
    pub hash: String,
    // Some tags for discovery and categorization
    // pub block_range: BlockRange,
}

/* File manifest */
#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, SimpleObject)]
pub struct FileManifest {
    pub total_bytes: u64,
    pub chunk_size: u64,
    pub chunk_hashes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, SimpleObject)]
pub struct FileManifestMeta {
    pub meta_info: FileMetaInfo,
    pub file_manifest: FileManifest,
}

/* Bundle - packaging of file manifests mapped into local files */
#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct Bundle {
    pub ipfs_hash: String,
    pub manifest: BundleManifest,
    /// IPFS hash, File manifest spec
    pub file_manifests: Vec<FileManifestMeta>,
}

#[derive(Clone, Debug)]
pub struct LocalBundle {
    pub bundle: Bundle,
    pub local_path: object_store::path::Path,
}

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct BlockRange {
    pub start_block: Option<u64>,
    pub end_block: Option<u64>,
}

/// Validate the bundle configurations at initialization
pub fn validate_bundle_entries(entries: Vec<String>) -> Result<Vec<(String, Path)>, Error> {
    let mut results = Vec::new();

    for entry in entries {
        results.push(parse_bundle_entry(entry)?);
    }

    Ok(results)
}

/// Validate the file configurations at initialization
pub fn validate_file_entries(entries: Vec<String>) -> Result<Vec<(String, Path)>, Error> {
    let mut results = Vec::new();

    for entry in entries {
        results.push(parse_file_entry(entry)?);
    }

    Ok(results)
}

/// Bundle entry must be in the format of "valid_ipfs_hash:valid_local_path"
pub fn parse_bundle_entry(entry: String) -> Result<(String, Path), Error> {
    let parts: Vec<&str> = entry.split(':').collect();
    if parts.len() != 2 {
        return Err(Error::InvalidConfig(format!(
            "Invalid format for bundle entry: {}",
            entry
        )));
    }

    let ipfs_hash = parts[0];
    let local_path = parts[1];
    if !is_valid_ipfs_hash(ipfs_hash) {
        return Err(Error::InvalidConfig(format!(
            "Invalid IPFS hash: {}",
            ipfs_hash
        )));
    }

    Ok((ipfs_hash.to_string(), Path::from(local_path)))
}

/// Bundle entry must be in the format of "valid_ipfs_hash:valid_local_path"
pub fn parse_file_entry(entry: String) -> Result<(String, Path), Error> {
    let parts: Vec<&str> = entry.split(':').collect();
    if parts.len() != 2 {
        return Err(Error::InvalidConfig(format!(
            "Invalid format for file entry: {}",
            entry
        )));
    }

    let ipfs_hash = parts[0];
    let file_name = parts[1];
    if !is_valid_ipfs_hash(ipfs_hash) {
        return Err(Error::InvalidConfig(format!(
            "Invalid IPFS hash: {}",
            ipfs_hash
        )));
    }
    Ok((ipfs_hash.to_string(), Path::from(file_name)))
}
