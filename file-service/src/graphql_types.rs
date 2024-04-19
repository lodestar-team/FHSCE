use async_graphql::SimpleObject;
use file_exchange::manifest::{
    Bundle, BundleManifest, FileManifest, FileManifestMeta, FileMetaInfo,
};
use serde::{Deserialize, Serialize};

//TODO: would be better to find a way to inherit the GraphQL types from file-exchange crate

/* Manifest types with GraphQL type derivation */
#[derive(Clone, Debug, SimpleObject)]
pub struct GraphQlBundleManifest {
    pub files: Vec<GraphQlFileMetaInfo>,
    pub file_type: Option<String>,
    pub spec_version: Option<String>,
    pub description: Option<String>,
    pub chain_id: Option<String>,
}

impl From<BundleManifest> for GraphQlBundleManifest {
    fn from(manifest: BundleManifest) -> Self {
        Self {
            files: manifest
                .files
                .into_iter()
                .map(GraphQlFileMetaInfo::from)
                .collect(),
            file_type: manifest.file_type,
            spec_version: manifest.spec_version,
            description: manifest.description,
            chain_id: manifest.chain_id,
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct GraphQlBundle {
    pub ipfs_hash: String,
    pub manifest: GraphQlBundleManifest,
    pub file_manifests: Vec<GraphQlFileManifestMeta>,
}

impl From<Bundle> for GraphQlBundle {
    fn from(bundle: Bundle) -> Self {
        Self {
            ipfs_hash: bundle.ipfs_hash,
            manifest: GraphQlBundleManifest::from(bundle.manifest),
            file_manifests: bundle
                .file_manifests
                .into_iter()
                .map(GraphQlFileManifestMeta::from)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct GraphQlFileManifestMeta {
    pub meta_info: GraphQlFileMetaInfo,
    pub file_manifest: GraphQlFileManifest,
}

impl From<FileManifestMeta> for GraphQlFileManifestMeta {
    fn from(meta: FileManifestMeta) -> Self {
        Self {
            meta_info: GraphQlFileMetaInfo::from(meta.meta_info),
            file_manifest: GraphQlFileManifest::from(meta.file_manifest),
        }
    }
}

#[derive(Clone, Debug, SimpleObject)]
pub struct GraphQlFileMetaInfo {
    pub name: String,
    pub hash: String,
}

impl From<FileMetaInfo> for GraphQlFileMetaInfo {
    fn from(manifest: FileMetaInfo) -> Self {
        Self {
            name: manifest.name,
            hash: manifest.hash,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GraphQlFileManifest {
    pub total_bytes: u64,
    pub chunk_size: u64,
    pub chunk_hashes: Vec<String>,
}

impl From<FileManifest> for GraphQlFileManifest {
    fn from(manifest: FileManifest) -> Self {
        Self {
            total_bytes: manifest.total_bytes,
            chunk_size: manifest.chunk_size,
            chunk_hashes: manifest.chunk_hashes,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct GraphQlCostModel {
    pub deployment: String,
    pub price_per_byte: f64,
}
