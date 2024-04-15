use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;

use crate::graphql_types::{GraphQlBundle, GraphQlFileManifestMeta};
use file_exchange::manifest::{Bundle, FileManifestMeta};

use super::ServerContext;

#[derive(Default)]
pub struct StatusQuery;

#[Object]
impl StatusQuery {
    /// Files inside some bundles
    async fn bundled_files(
        &self,
        ctx: &Context<'_>,
        deployments: Option<Vec<String>>,
    ) -> Result<Vec<GraphQlFileManifestMeta>, anyhow::Error> {
        let bundles: Vec<Bundle> = ctx
            .data_unchecked::<ServerContext>()
            .state
            .bundles
            .lock()
            .await
            .values()
            .map(|b| b.bundle.clone())
            .collect();
        let file_metas: Vec<FileManifestMeta> = bundles
            .iter()
            .flat_map(|b| b.file_manifests.clone())
            .collect();

        if deployments.is_none() {
            return Ok(file_metas
                .iter()
                .map(|m| GraphQlFileManifestMeta::from(m.clone()))
                .collect::<Vec<GraphQlFileManifestMeta>>());
        };
        let ids = deployments.unwrap();
        Ok(file_metas
            .iter()
            .filter(|m| ids.contains(&m.meta_info.hash))
            .cloned()
            .map(GraphQlFileManifestMeta::from)
            .collect())
    }

    /// A file inside some bundles
    async fn bundled_file(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlFileManifestMeta>, anyhow::Error> {
        let bundles: Vec<Bundle> = ctx
            .data_unchecked::<ServerContext>()
            .state
            .bundles
            .lock()
            .await
            .values()
            .map(|b| b.bundle.clone())
            .collect();
        let file_metas: Vec<FileManifestMeta> = bundles
            .iter()
            .flat_map(|b| b.file_manifests.clone())
            .collect();
        let manifest_graphql = file_metas
            .iter()
            .find(|m| m.meta_info.hash == deployment)
            .cloned()
            .map(GraphQlFileManifestMeta::from);

        Ok(manifest_graphql)
    }

    /// Bundles, optional deployments filter
    async fn bundles(
        &self,
        ctx: &Context<'_>,
        deployments: Option<Vec<String>>,
    ) -> Result<Vec<GraphQlBundle>, anyhow::Error> {
        tracing::trace!("received bundles request");
        let all_bundles = &ctx
            .data_unchecked::<ServerContext>()
            .state
            .bundles
            .lock()
            .await
            .clone();

        let bundles = if deployments.is_none() {
            tracing::trace!(
                bundles = tracing::field::debug(&all_bundles),
                "no deployment filter"
            );
            all_bundles
                .values()
                .cloned()
                .map(|b| GraphQlBundle::from(b.bundle))
                .collect()
        } else {
            let ids = deployments.unwrap();
            ids.iter()
                .filter_map(|key| all_bundles.get(key))
                .cloned()
                .map(|b| GraphQlBundle::from(b.bundle))
                .collect()
        };
        tracing::debug!(bundles = tracing::field::debug(&bundles), "queried bundles");
        Ok(bundles)
    }

    /// A single bundle by deployment hash
    async fn bundle(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlBundle>, anyhow::Error> {
        tracing::trace!("received bundle request");
        // let deployment_id = DeploymentId::from_str(&deployment)?;
        let bundle: Option<Bundle> = ctx
            .data_unchecked::<ServerContext>()
            .state
            .bundles
            .lock()
            .await
            .get(&deployment)
            .map(|b| b.bundle.clone());

        Ok(bundle.map(GraphQlBundle::from))
    }

    /// Serving files with optional deployments filter
    async fn files(
        &self,
        ctx: &Context<'_>,
        deployments: Option<Vec<String>>,
    ) -> Result<Vec<GraphQlFileManifestMeta>, anyhow::Error> {
        let file_metas: Vec<FileManifestMeta> = ctx
            .data_unchecked::<ServerContext>()
            .state
            .files
            .lock()
            .await
            .values()
            .cloned()
            .collect();

        if deployments.is_none() {
            return Ok(file_metas
                .iter()
                .map(|m| GraphQlFileManifestMeta::from(m.clone()))
                .collect::<Vec<GraphQlFileManifestMeta>>());
        };
        let ids = deployments.unwrap();
        Ok(file_metas
            .iter()
            .filter(|m| ids.contains(&m.meta_info.hash))
            .cloned()
            .map(GraphQlFileManifestMeta::from)
            .collect())
    }

    /// A single file by deployment hash
    async fn file(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlFileManifestMeta>, anyhow::Error> {
        let file_meta: Option<GraphQlFileManifestMeta> = ctx
            .data_unchecked::<ServerContext>()
            .state
            .files
            .lock()
            .await
            .values()
            .find(|m| m.meta_info.hash == deployment)
            .cloned()
            .map(GraphQlFileManifestMeta::from);

        Ok(file_meta)
    }
}

pub type StatusSchema = Schema<StatusQuery, EmptyMutation, EmptySubscription>;

pub async fn build_schema() -> StatusSchema {
    Schema::build(StatusQuery, EmptyMutation, EmptySubscription).finish()
}

pub async fn status(State(context): State<ServerContext>, req: GraphQLRequest) -> GraphQLResponse {
    context
        .state
        .status_schema
        .execute(req.into_inner().data(context.clone()))
        .await
        .into()
}
