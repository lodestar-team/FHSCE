use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

use serde::Deserializer;

use super::Query;
use crate::{errors::Error, graphql::graphql_query};

// Query current epoch from network subgraph
pub async fn current_epoch(
    graphql_client: &reqwest::Client,
    network_subgraph: &str,
    graph_network_id: u64,
) -> Result<u64, anyhow::Error> {
    // Types for deserializing the network subgraph response
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GraphNetworkData {
        graph_network: Option<GraphNetwork>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GraphNetwork {
        current_epoch: u64,
    }

    // Query the current epoch
    let query = r#"query epoch($id: ID!) { graphNetwork(id: $id) { currentEpoch } }"#;
    let result = graphql_query::<GraphNetworkData>(
        graphql_client,
        network_subgraph,
        Query::new_with_variables(query, [("id", graph_network_id.into())]),
    )
    .await?;

    result?
        .graph_network
        .ok_or_else(|| anyhow::anyhow!("Network {} not found", graph_network_id))
        .map(|network| network.current_epoch)
}

/// List an indexer's active allocations.
pub async fn indexer_active_allocations(
    client: &reqwest::Client,
    network_subgraph: &str,
    indexer_address: Address,
) -> Result<Vec<Allocation>, Error> {
    // Types for deserializing the network subgraph response
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct IndexerAllocationsResponse {
        indexer: Option<Indexer>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Indexer {
        active_allocations: Vec<Allocation>,
    }

    let query = r#"
        query allocations($indexer: ID!) {
            indexer(id: $indexer) {
                activeAllocations: totalAllocations(
                    where: { status: Active }
                    orderDirection: desc
                    first: 1000
                ) {
                    id
                    subgraphDeployment {
                        id
                        ipfsHash
                    }
                }
            }
        }
        "#;

    let response = graphql_query::<IndexerAllocationsResponse>(
        client,
        network_subgraph,
        Query::new_with_variables(query, [("indexer", format!("{indexer_address:?}").into())]),
    )
    .await
    .map_err(|e| Error::DataUnavailable(e.to_string()))?;

    let indexer = response
        .map_err(|e| Error::DataUnavailable(e.to_string()))
        .and_then(|data| {
            data.indexer.ok_or_else(|| {
                Error::DataUnavailable(format!(
                    "Indexer `{indexer_address}` not found on the network"
                ))
            })
        })?;

    Ok(indexer.active_allocations)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Allocation {
    pub id: Address,
    pub subgraph_deployment: SubgraphDeployment,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SubgraphDeployment {
    pub id: String,
    #[serde(rename = "ipfsHash")]
    pub ipfs_hash: String,
}

impl<'d> Deserialize<'d> for Allocation {
    fn deserialize<D>(deserializer: D) -> Result<Allocation, D::Error>
    where
        D: Deserializer<'d>,
    {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct InnerIndexer {
            id: Address,
        }

        #[derive(Deserialize)]
        #[allow(non_snake_case)]
        struct Outer {
            id: Address,
            subgraphDeployment: SubgraphDeployment,
        }

        let outer = Outer::deserialize(deserializer)?;

        Ok(Allocation {
            id: outer.id,
            subgraph_deployment: outer.subgraphDeployment,
        })
    }
}
