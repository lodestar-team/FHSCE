use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::{Context, EmptySubscription, MergedObject, Object, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQL};
use axum::{extract::State, routing::get, Router, serve};
use file_exchange::{
    config::{BundleArgs, PublisherArgs},
    publisher::ManifestPublisher,
};
use core::net::SocketAddr;
use http::HeaderMap;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::file_server::{util::graphql_playground, FileServiceError, ServerContext};
use crate::graphql_types::{GraphQlBundle, GraphQlCostModel, GraphQlFileManifestMeta};
use file_exchange::{
    errors::{Error, ServerError},
    manifest::{
        ipfs::IpfsClient,
        manifest_fetcher::{fetch_file_manifest_from_ipfs, read_bundle},
        store::Store,
        Bundle, FileManifestMeta, FileMetaInfo, LocalBundle,
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

pub fn serve_admin(context: ServerContext)  {
    tokio::spawn(async move {
        let admin_schema=  build_schema().await;
        let admin_context = AdminContext::new(
            AdminState {
                client: context.state.client.clone(),
                bundles: context.state.bundles.clone(),
                files: context.state.files.clone(),
                prices: context.state.prices.clone(),
                admin_auth_token: context.state.admin_auth_token.clone(),
                admin_schema: admin_schema.clone(),
                store: context.state.store.clone(),
            }
            .into(),
        );
        let addr = context.state.config.server.admin_host_and_port;
        tracing::info!(address = %addr
            , "Serve admin metrics");

        let router = Router::new()
            .route("/admin", get(graphql_playground).post_service(GraphQL::new(admin_schema)))
            .with_state(admin_context);

        let listener = TcpListener::bind(&addr)
            .await
            .expect("Failed to bind to file-service admin port");
        serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await.expect("Failed to initialize admin server");
        // Server::bind(&addr)
        //     .serve(router.into_make_service())
        //     .await
        //     .expect("Failed to initialize admin server")
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
    // Publish a bundle; the location of files are relative to the main directory of the server
    #[allow(clippy::too_many_arguments)]
    async fn publish_and_serve_bundle(
        &self,
        ctx: &Context<'_>,
        filenames: Vec<String>,
        prefixes: Option<Vec<String>>,
        chunk_size: Option<u64>,
        bundle_name: Option<String>,
        file_type: Option<String>,
        bundle_version: Option<String>,
        identifier: Option<String>,
        start_block: Option<u64>,
        end_block: Option<u64>,
        description: Option<String>,
        chain_id: Option<String>,
    ) -> Result<GraphQlBundle, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
        }
        // publish bundle
        let client = ctx.data_unchecked::<AdminContext>().state.client.clone();
        let publisher = ManifestPublisher::new(
            client,
            PublisherArgs {
                chunk_size: chunk_size.unwrap_or(1048576),
                filenames,
                prefixes: prefixes.unwrap_or_default(),
                bundle: Some(BundleArgs {
                    bundle_name,
                    file_type,
                    bundle_version,
                    identifier,
                    start_block,
                    end_block,
                    description,
                    chain_id,
                }),
                storage_method: ctx
                    .data_unchecked::<AdminContext>()
                    .state
                    .store
                    .storage_method
                    .clone(),
                ..Default::default()
            },
        );

        let published = publisher
            .publish()
            .await
            .map_err(|e| ServerError::ContextError(e.to_string()))?;
        let deployment = published
            .first()
            .ok_or(ServerError::ContextError("No bundle published".to_string()))?;

        let bundle = match read_bundle(
            &ctx.data_unchecked::<AdminContext>().state.client,
            deployment,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => return Err(ServerError::RequestBodyError(e.to_string())),
        };
        let local_bundle = LocalBundle {
            bundle: bundle.clone(),
            //TODO: remove this field
            local_path: "".into(),
        };

        ctx.data_unchecked::<AdminContext>()
            .state
            .bundles
            .lock()
            .await
            .insert(bundle.ipfs_hash.clone(), local_bundle);

        Ok(GraphQlBundle::from(bundle))
    }

    // Add a bundle
    async fn add_bundle(
        &self,
        ctx: &Context<'_>,
        deployment: String,
        location: String,
    ) -> Result<GraphQlBundle, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
        }
        let bundle = match read_bundle(
            &ctx.data_unchecked::<AdminContext>().state.client,
            &deployment,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => return Err(ServerError::RequestBodyError(e.to_string())),
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
            .await
            .map_err(|e| ServerError::ContextError(e.to_string()))?;

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
    ) -> Result<Vec<GraphQlBundle>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(
                "Failed to authenticate".to_string(),
            ));
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
                        .map_err(|e| ServerError::RequestBodyError(e.to_string()))?;

                    let local_bundle = LocalBundle {
                        bundle: bundle.clone(),
                        local_path: location.into(),
                    };
                    let _ = ctx
                        .data_unchecked::<AdminContext>()
                        .state
                        .store
                        .validate_local_bundle(&local_bundle)
                        .await
                        .map_err(|e| ServerError::ContextError(e.to_string()))?;
                    bundle_ref
                        .clone()
                        .lock()
                        .await
                        .insert(bundle.ipfs_hash.clone(), local_bundle);

                    Ok::<_, crate::admin::ServerError>(GraphQlBundle::from(bundle))
                }
            })
            .collect::<Vec<_>>();

        // Since collect() gathers futures, we need to resolve them. You can use `try_join_all` for this.
        let resolved_bundles: Vec<GraphQlBundle> = futures::future::try_join_all(bundles).await?;

        Ok(resolved_bundles)
    }

    async fn remove_bundle(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlBundle>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
    ) -> Result<Vec<GraphQlBundle>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
                    .ok_or(ServerError::ContextError(format!(
                        "Deployment not found: {}",
                        deployment
                    )))
            })
            .collect::<Vec<_>>();

        let removed_bundles: Vec<GraphQlBundle> = futures::future::try_join_all(bundles).await?;

        Ok(removed_bundles)
    }

    // Add a file
    async fn add_file(
        &self,
        ctx: &Context<'_>,
        deployment: String,
        file_name: String,
    ) -> Result<GraphQlFileManifestMeta, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
        }
        let file_manifest = match fetch_file_manifest_from_ipfs(
            &ctx.data_unchecked::<AdminContext>().state.client,
            &deployment,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => return Err(ServerError::ContextError(e.to_string())),
        };

        let meta = FileManifestMeta {
            meta_info: FileMetaInfo {
                name: file_name.clone(),
                hash: deployment.clone(),
            },
            file_manifest,
        };
        ctx.data_unchecked::<AdminContext>()
            .state
            .store
            .read_and_validate_file(&meta, None)
            .await
            .map_err(|e| ServerError::ContextError(e.to_string()))?;
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
    ) -> Result<Vec<GraphQlFileManifestMeta>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
                        .map_err(|e| ServerError::ContextError(e.to_string()))?;

                    let meta = FileManifestMeta {
                        meta_info: FileMetaInfo {
                            name: file_name.clone(),
                            hash: deployment.clone(),
                        },
                        file_manifest,
                    };
                    ctx.data_unchecked::<AdminContext>()
                        .state
                        .store
                        .read_and_validate_file(&meta, None)
                        .await
                        .map_err(|e| ServerError::ContextError(e.to_string()))?;
                    file_ref
                        .clone()
                        .lock()
                        .await
                        .insert(deployment.clone(), meta.clone());

                    Ok::<_, crate::admin::ServerError>(GraphQlFileManifestMeta::from(meta))
                }
            })
            .collect::<Vec<_>>();

        // Since collect() gathers futures, we need to resolve them. You can use `try_join_all` for this.
        let resolved_files: Result<Vec<GraphQlFileManifestMeta>, _> =
            futures::future::try_join_all(files).await;

        Ok(resolved_files.unwrap_or_default())
    }

    // Publish a bundle; the location of files are relative to the main directory of the server
    async fn publish_and_serve_files(
        &self,
        ctx: &Context<'_>,
        filenames: Vec<String>,
        prefixes: Option<Vec<String>>,
        chunk_size: Option<u64>,
    ) -> Result<Vec<GraphQlFileManifestMeta>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
        }
        // publish bundle
        let client = ctx.data_unchecked::<AdminContext>().state.client.clone();
        let file_ref = ctx.data_unchecked::<AdminContext>().state.files.clone();
        let publisher = ManifestPublisher::new(
            client.clone(),
            PublisherArgs {
                chunk_size: chunk_size.unwrap_or(1048576),
                filenames: filenames.clone(),
                prefixes: prefixes.unwrap_or_default(),
                storage_method: ctx
                    .data_unchecked::<AdminContext>()
                    .state
                    .store
                    .storage_method
                    .clone(),
                ..Default::default()
            },
        );

        let published = publisher
            .publish()
            .await
            .map_err(|e| ServerError::ContextError(e.to_string()))?;

        let files = published
            .iter()
            .zip(filenames)
            .map(|(deployment, file_name)| {
                let client = client.clone();
                let file_ref = file_ref.clone();

                async move {
                    tracing::debug!(deployment, file_name, "Adding file");

                    let file_manifest = fetch_file_manifest_from_ipfs(&client.clone(), deployment)
                        .await
                        .map_err(|e| ServerError::ContextError(e.to_string()))?;

                    let meta = FileManifestMeta {
                        meta_info: FileMetaInfo {
                            name: file_name.clone(),
                            hash: deployment.clone(),
                        },
                        file_manifest,
                    };
                    ctx.data_unchecked::<AdminContext>()
                        .state
                        .store
                        .read_and_validate_file(&meta, None)
                        .await
                        .map_err(|e| ServerError::ContextError(e.to_string()))?;
                    file_ref
                        .clone()
                        .lock()
                        .await
                        .insert(deployment.clone(), meta.clone());

                    Ok::<_, crate::admin::ServerError>(GraphQlFileManifestMeta::from(meta))
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
    ) -> Result<Option<GraphQlFileManifestMeta>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
    ) -> Result<Vec<GraphQlFileManifestMeta>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
                    .ok_or(ServerError::ContextError(format!(
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
    ) -> Result<GraphQlCostModel, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
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
    ) -> Result<Vec<GraphQlCostModel>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
    ) -> Result<Option<GraphQlCostModel>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
    ) -> Result<Vec<GraphQlCostModel>, ServerError> {
        if ctx.data_opt::<String>()
            != ctx
                .data_unchecked::<AdminContext>()
                .state
                .admin_auth_token
                .as_ref()
        {
            return Err(ServerError::InvalidAuthentication(format!(
                "Failed to authenticate: {:#?}",
                ctx.data_opt::<String>(),
            )));
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
                    .ok_or(ServerError::ContextError(format!(
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

/* StatusQuery and CostQuer are repeated from the status/cost endpoints
** This is due to the difference in ServerContext
** It isn't possible to use generic types for these queries due to async-graphql
** But we can later restructure admin/server contexts with the arc bundles/files/cost */
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
            .data_unchecked::<AdminContext>()
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
            .data_unchecked::<AdminContext>()
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
            .data_unchecked::<AdminContext>()
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
        let bundle: Option<Bundle> = ctx
            .data_unchecked::<AdminContext>()
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
            .data_unchecked::<AdminContext>()
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
            .data_unchecked::<AdminContext>()
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

#[derive(Default)]
pub struct PriceQuery;

#[Object]
impl PriceQuery {
    /// Provide an array of cost model to the queried deployment whether it is served or not
    async fn cost_models(
        &self,
        ctx: &Context<'_>,
        deployments: Vec<String>,
    ) -> Result<Vec<GraphQlCostModel>, anyhow::Error> {
        let mut cost_models = vec![];
        for deployment in deployments {
            let price: Option<f64> = ctx
                .data_unchecked::<AdminContext>()
                .state
                .prices
                .lock()
                .await
                .get(&deployment)
                .cloned();
            if let Some(p) = price {
                cost_models.push(GraphQlCostModel {
                    deployment,
                    price_per_byte: p,
                })
            }
        }
        Ok(cost_models)
    }

    /// provide a cost model for a specific file/bundle served
    async fn cost_model(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<Option<GraphQlCostModel>, anyhow::Error> {
        let model: Option<GraphQlCostModel> = ctx
            .data_unchecked::<AdminContext>()
            .state
            .prices
            .lock()
            .await
            .get(&deployment)
            .cloned()
            .map(|p| GraphQlCostModel {
                deployment,
                price_per_byte: p,
            });
        Ok(model)
    }
}
