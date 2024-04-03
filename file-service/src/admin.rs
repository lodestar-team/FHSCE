use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::{Context, EmptySubscription, MergedObject, Object, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{extract::State, routing::get, Router, Server};
use http::HeaderMap;
use tokio::sync::Mutex;

use crate::file_server::{
    cost::{GraphQlCostModel, PriceQuery},
    status::{GraphQlBundle, GraphQlFileManifestMeta, StatusQuery},
    util::graphql_playground,
    FileServiceError, ServerContext,
};
use file_exchange::{
    errors::{Error, ServerError},
    manifest::{
        ipfs::IpfsClient,
        manifest_fetcher::{fetch_file_manifest_from_ipfs, read_bundle},
        store::Store,
        FileManifestMeta, FileMetaInfo, LocalBundle,
    },
};

#[derive(Clone)]
pub struct AdminState {
    pub client: IpfsClient,
    pub bundles: Arc<Mutex<HashMap<String, LocalBundle>>>,
    pub files: Arc<Mutex<HashMap<String, FileManifestMeta>>>,
    pub prices: Arc<Mutex<HashMap<String, f64>>>,
    pub admin_auth_token: Option<String>,
    pub admin_schema: AdminSchema,
    pub store: Store,
}

#[derive(Clone)]
pub struct AdminContext {
    pub state: Arc<AdminState>,
}

impl AdminContext {
    pub fn new(state: Arc<AdminState>) -> Self {
        Self { state }
    }
}

#[derive(MergedObject, Default)]
pub struct MergedQuery(StatusQuery, PriceQuery);

#[derive(MergedObject, Default)]
pub struct MergedMutation(StatusMutation, PriceMutation);

pub type AdminSchema = Schema<MergedQuery, MergedMutation, EmptySubscription>;

pub async fn build_schema() -> AdminSchema {
    Schema::build(
        MergedQuery(StatusQuery, PriceQuery),
        MergedMutation(StatusMutation, PriceMutation),
        EmptySubscription,
    )
    .finish()
}

fn get_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().map(|s| s.to_string()).ok())
}

// GraphQL handler to update status schema
//TODO: add mutation query fn for on-chain management?
async fn graphql_handler(
    State(context): State<AdminContext>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let mut req = req.into_inner().data(context.clone());
    if let Some(token) = get_token_from_headers(&headers) {
        req = req.data(token);
    }
    context.state.admin_schema.execute(req).await.into()
}

pub fn serve_admin(context: ServerContext) {
    tokio::spawn(async move {
        let admin_context = AdminContext::new(
            AdminState {
                client: context.state.client.clone(),
                bundles: context.state.bundles.clone(),
                files: context.state.files.clone(),
                prices: context.state.prices.clone(),
                admin_auth_token: context.state.admin_auth_token.clone(),
                admin_schema: build_schema().await,
                store: context.state.store.clone(),
            }
            .into(),
        );
        tracing::info!(address = %context.state.config.server.admin_host_and_port, "Serve admin metrics");

        let router = Router::new()
            .route("/admin", get(graphql_playground).post(graphql_handler))
            .with_state(admin_context);

        Server::bind(&context.state.config.server.admin_host_and_port)
            .serve(router.into_make_service())
            .await
            .expect("Failed to initialize admin server")
    });
}

/// Create an admin error response
pub fn admin_error_response(msg: &str) -> FileServiceError {
    FileServiceError::AdminError(Error::ServerError(ServerError::InvalidAuthentication(
        msg.to_string(),
    )))
}

#[derive(Default)]
pub struct StatusMutation;

#[Object]
impl StatusMutation {
    // Add a bundle
    async fn add_bundle(
        &self,
        ctx: &Context<'_>,
        deployment: String,
        location: String,
    ) -> Result<GraphQlBundle, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!(format!(
                "Failed to authenticate: {:#?} (admin: {:#?}",
                ctx.data_opt::<String>(),
                ctx.data_unchecked::<AdminContext>()
                    .state
                    .admin_auth_token
                    .as_ref()
            )));
        }
        let bundle = match read_bundle(
            &ctx.data_unchecked::<AdminContext>().state.client,
            &deployment,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => return Err(anyhow::anyhow!(e.to_string(),)),
        };
        let local_bundle = LocalBundle {
            bundle: bundle.clone(),
            local_path: location.into(),
        };
        let _ = ctx
            .data_unchecked::<AdminContext>()
            .state
            .store
            .validate_local_bundle(&local_bundle)
            .await;

        ctx.data_unchecked::<AdminContext>()
            .state
            .bundles
            .lock()
            .await
            .insert(bundle.ipfs_hash.clone(), local_bundle);

        Ok(GraphQlBundle::from(bundle))
    }

    // Add multiple bundles
    async fn add_bundles(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
        locations: Vec<String>,
    ) -> Result<Vec<GraphQlBundle>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }
        let client = ctx.data_unchecked::<AdminContext>().state.client.clone();
        let bundle_ref = ctx.data_unchecked::<AdminContext>().state.bundles.clone();
        let bundles = deployments
            .iter()
            .zip(locations)
            .map(|(deployment, location)| {
                let client = client.clone();
                let bundle_ref = bundle_ref.clone();

                async move {
                    tracing::debug!(deployment, location, "Adding bundle");

                    let bundle = read_bundle(&client.clone(), deployment)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;

                    let local_bundle = LocalBundle {
                        bundle: bundle.clone(),
                        local_path: location.into(),
                    };
                    let _ = ctx
                        .data_unchecked::<AdminContext>()
                        .state
                        .store
                        .validate_local_bundle(&local_bundle)
                        .await;
                    bundle_ref
                        .clone()
                        .lock()
                        .await
                        .insert(bundle.ipfs_hash.clone(), local_bundle);

                    Ok::<_, anyhow::Error>(GraphQlBundle::from(bundle))
                }
            })
            .collect::<Vec<_>>();

        // Since collect() gathers futures, we need to resolve them. You can use `try_join_all` for this.
        let resolved_bundles: Result<Vec<GraphQlBundle>, _> =
            futures::future::try_join_all(bundles).await;

        Ok(resolved_bundles.unwrap_or_default())
    }

    async fn remove_bundle(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlBundle>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }

        let bundle = ctx
            .data_unchecked::<AdminContext>()
            .state
            .bundles
            .lock()
            .await
            .remove(&deployment)
            .map(|b| GraphQlBundle::from(b.bundle));

        Ok(bundle)
    }

    async fn remove_bundles(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
    ) -> Result<Vec<GraphQlBundle>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }

        let bundles = deployments
            .iter()
            .map(|deployment| async move {
                ctx.data_unchecked::<AdminContext>()
                    .state
                    .bundles
                    .lock()
                    .await
                    .remove(deployment)
                    .map(|b| GraphQlBundle::from(b.bundle))
                    .ok_or(anyhow::anyhow!(format!(
                        "Deployment not found: {}",
                        deployment
                    )))
            })
            .collect::<Vec<_>>();

        let removed_bundles: Result<Vec<GraphQlBundle>, _> =
            futures::future::try_join_all(bundles).await;

        removed_bundles
    }

    // Add a file
    async fn add_file(
        &self,
        ctx: &Context<'_>,
        deployment: String,
        file_name: String,
    ) -> Result<GraphQlFileManifestMeta, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!(format!(
                "Failed to authenticate: {:#?} (admin: {:#?}",
                ctx.data_opt::<String>(),
                ctx.data_unchecked::<AdminContext>()
                    .state
                    .admin_auth_token
                    .as_ref()
            )));
        }
        let file_manifest = match fetch_file_manifest_from_ipfs(
            &ctx.data_unchecked::<AdminContext>().state.client,
            &deployment,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => return Err(anyhow::anyhow!(e.to_string(),)),
        };

        let meta = FileManifestMeta {
            meta_info: FileMetaInfo {
                name: file_name.clone(),
                hash: deployment.clone(),
            },
            file_manifest,
        };
        let _ = ctx
            .data_unchecked::<AdminContext>()
            .state
            .store
            .read_and_validate_file(&meta, None)
            .await;
        ctx.data_unchecked::<AdminContext>()
            .state
            .files
            .lock()
            .await
            .insert(deployment.clone(), meta.clone());

        Ok(GraphQlFileManifestMeta::from(meta))
    }

    // Add multiple files
    async fn add_files(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
        file_names: Vec<String>,
    ) -> Result<Vec<GraphQlFileManifestMeta>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }
        let client = ctx.data_unchecked::<AdminContext>().state.client.clone();
        let file_ref = ctx.data_unchecked::<AdminContext>().state.files.clone();
        let files = deployments
            .iter()
            .zip(file_names)
            .map(|(deployment, file_name)| {
                let client = client.clone();
                let file_ref = file_ref.clone();

                async move {
                    tracing::debug!(deployment, file_name, "Adding file");

                    let file_manifest = fetch_file_manifest_from_ipfs(&client.clone(), deployment)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;

                    let meta = FileManifestMeta {
                        meta_info: FileMetaInfo {
                            name: file_name.clone(),
                            hash: deployment.clone(),
                        },
                        file_manifest,
                    };
                    let _ = ctx
                        .data_unchecked::<AdminContext>()
                        .state
                        .store
                        .read_and_validate_file(&meta, None)
                        .await;
                    file_ref
                        .clone()
                        .lock()
                        .await
                        .insert(deployment.clone(), meta.clone());

                    Ok::<_, anyhow::Error>(GraphQlFileManifestMeta::from(meta))
                }
            })
            .collect::<Vec<_>>();

        // Since collect() gathers futures, we need to resolve them. You can use `try_join_all` for this.
        let resolved_files: Result<Vec<GraphQlFileManifestMeta>, _> =
            futures::future::try_join_all(files).await;

        Ok(resolved_files.unwrap_or_default())
    }

    async fn remove_file(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlFileManifestMeta>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }

        let file = ctx
            .data_unchecked::<AdminContext>()
            .state
            .files
            .lock()
            .await
            .remove(&deployment)
            .map(GraphQlFileManifestMeta::from);

        Ok(file)
    }

    async fn remove_files(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
    ) -> Result<Vec<GraphQlFileManifestMeta>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }

        let files = deployments
            .iter()
            .map(|deployment| async move {
                ctx.data_unchecked::<AdminContext>()
                    .state
                    .files
                    .lock()
                    .await
                    .remove(deployment)
                    .map(GraphQlFileManifestMeta::from)
                    .ok_or(anyhow::anyhow!(format!(
                        "Deployment not found: {}",
                        deployment
                    )))
            })
            .collect::<Vec<_>>();

        let removed_files: Result<Vec<GraphQlFileManifestMeta>, _> =
            futures::future::try_join_all(files).await;

        removed_files
    }
}

#[derive(Default)]
pub struct PriceMutation;

#[Object]
impl PriceMutation {
    /// Set price for a deployment
    async fn set_price(
        &self,
        ctx: &Context<'_>,
        deployment: String,
        price_per_byte: f64,
    ) -> Result<GraphQlCostModel, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!(format!(
                "Failed to authenticate: {:#?} (admin: {:#?}",
                ctx.data_opt::<String>(),
                ctx.data_unchecked::<AdminContext>()
                    .state
                    .admin_auth_token
                    .as_ref()
            )));
        }

        ctx.data_unchecked::<AdminContext>()
            .state
            .prices
            .lock()
            .await
            .insert(deployment.clone(), price_per_byte);

        Ok(GraphQlCostModel {
            deployment,
            price_per_byte,
        })
    }

    /// Set multiple prices
    async fn set_prices(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
        prices: Vec<f64>,
    ) -> Result<Vec<GraphQlCostModel>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }
        let price_ref = ctx.data_unchecked::<AdminContext>().state.prices.clone();
        let prices = deployments
            .iter()
            .zip(prices)
            .map(|(deployment, price)| {
                let price_ref = price_ref.clone();

                async move {
                    price_ref
                        .clone()
                        .lock()
                        .await
                        .insert(deployment.clone(), price);

                    Ok::<_, anyhow::Error>(GraphQlCostModel {
                        deployment: deployment.to_string(),
                        price_per_byte: price,
                    })
                }
            })
            .collect::<Vec<_>>();

        // Since collect() gathers futures, we need to resolve them. You can use `try_join_all` for this.
        let resolved_prices: Result<Vec<GraphQlCostModel>, _> =
            futures::future::try_join_all(prices).await;

        Ok(resolved_prices.unwrap_or_default())
    }

    /// Removing the set price; default will be used as a fallback
    async fn remove_price(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlCostModel>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }

        let bundle = ctx
            .data_unchecked::<AdminContext>()
            .state
            .prices
            .lock()
            .await
            .remove(&deployment)
            .map(|price| GraphQlCostModel {
                deployment,
                price_per_byte: price,
            });

        Ok(bundle)
    }

    /// Remove set prices, default will be used later
    async fn remove_prices(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
    ) -> Result<Vec<GraphQlCostModel>, anyhow::Error> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(anyhow::anyhow!("Failed to authenticate"));
        }

        let prices = deployments
            .iter()
            .map(|deployment| async move {
                ctx.data_unchecked::<AdminContext>()
                    .state
                    .prices
                    .lock()
                    .await
                    .remove(deployment)
                    .map(|price| GraphQlCostModel {
                        deployment: deployment.to_string(),
                        price_per_byte: price,
                    })
                    .ok_or(anyhow::anyhow!(format!(
                        "Deployment not found: {}",
                        deployment
                    )))
            })
            .collect::<Vec<_>>();

        let removed_prices: Result<Vec<GraphQlCostModel>, _> =
            futures::future::try_join_all(prices).await;

        removed_prices
    }
}
