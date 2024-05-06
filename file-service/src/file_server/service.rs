// #![cfg(feature = "acceptor")]

use file_exchange::errors::Error;
use indexer_common::indexer_service::http::IndexerServiceImpl;
use thegraph::types::DeploymentId;
// #![cfg(feature = "acceptor")]
// use hyper_rustls::TlsAcceptor;
use crate::file_server::StatusCode;
use axum::{body::Body, response::Response};

use super::{
    bundle_containing_file,
    range::{parse_range_header, serve_file, serve_file_range},
    ServerContext,
};

// Serve file requests
pub async fn file_service(
    id: DeploymentId,
    req: &<ServerContext as IndexerServiceImpl>::Request,
    context: &ServerContext,
) -> Result<Response<Body>, Error> {
    tracing::debug!(
        id = tracing::field::debug(&id.to_string()),
        "Received file range request"
    );

    let local_bundle = context
        .state
        .bundles
        .lock()
        .await
        .get(&id.to_string())
        .cloned();
    let local_bundle = match local_bundle {
        Some(s) => s.clone(),
        None => {
            // not matched at bundle level, try match at file level
            let bundle = bundle_containing_file(context.state.bundles.clone(), &id).await;
            if let Some(bundle) = bundle {
                bundle.clone()
            } else {
                return Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body("Bundle not found".into())
                    .unwrap());
            }
        }
    };
    tracing::debug!(
        local_bundle = tracing::field::debug(&local_bundle),
        "Matched bundle"
    );

    match req.get("file-hash") {
        Some(hash) if hash.as_str().is_some() => {
            let file_manifest = match local_bundle
                .bundle
                .file_manifests
                .iter()
                .find(|file| file.meta_info.hash == hash.as_str().unwrap())
            {
                Some(c) => c,
                None => {
                    return Ok(Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body("File manifest not found".into())
                        .unwrap())
                }
            };
            // Parse the range header to get the start and end bytes
            match req.get("content-range") {
                Some(r) => {
                    let range = parse_range_header(r)?;
                    serve_file_range(
                        context.state.store.clone(),
                        &file_manifest.meta_info.name,
                        &local_bundle.local_path,
                        range,
                    )
                    .await
                }
                None => {
                    serve_file(
                        context.state.store.clone(),
                        &file_manifest.meta_info.name,
                        &local_bundle.local_path,
                    )
                    .await
                }
            }
        }
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_ACCEPTABLE)
            .body("Missing required file_manifest_hash header".into())
            .unwrap()),
    }
}
