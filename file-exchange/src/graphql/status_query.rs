use anyhow::Result;
use async_graphql::SimpleObject;
use serde::{Deserialize, Serialize};

use crate::errors::Error;

use super::{graphql_query, Query};

#[derive(Clone, Debug, SimpleObject, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlFile {
    pub hash: String,
}

#[derive(Clone, Debug, SimpleObject, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlFileMeta {
    pub meta_info: GraphQlFile,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct FileStatus {
    files: Vec<GraphQlFileMeta>,
}

#[derive(Clone, Debug, SimpleObject, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlBundle {
    pub ipfs_hash: String,
}
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct BundleStatus {
    bundles: Vec<GraphQlBundle>,
}

// Indexer files
pub async fn indexer_files(client: &reqwest::Client, url: &str) -> Result<Vec<String>, Error> {
    let status_url = format!("{}/files-status", url);
    let query = r#"query{files{metaInfo{hash}}}"#;
    let result = graphql_query::<FileStatus>(client, &status_url, Query::new(query)).await?;

    Ok(result
        .map_err(Error::GraphQLResponseError)?
        .files
        .iter()
        .map(|file| file.meta_info.hash.clone())
        .collect::<Vec<String>>())
}

// Indexer bundles
//TODO: find how to generate bundle GraphQL type shareable to file-service
//so values can be parsed more elegantly
pub async fn indexer_bundles(client: &reqwest::Client, url: &str) -> Result<Vec<String>, Error> {
    let status_url = format!("{}/files-status", url);
    let query = r#"query{bundles{ipfsHash}}"#;
    let result = graphql_query::<BundleStatus>(client, &status_url, Query::new(query)).await?;

    Ok(result
        .map_err(Error::GraphQLResponseError)?
        .bundles
        .iter()
        .map(|bundle| bundle.ipfs_hash.clone())
        .collect::<Vec<String>>())
}
