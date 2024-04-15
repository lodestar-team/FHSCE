use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::State;

use crate::file_server::ServerContext;
use crate::graphql_types::GraphQlCostModel;

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
        let price: f64 = ctx
            .data_unchecked::<ServerContext>()
            .state
            .config
            .server
            .default_price_per_byte;
        let cost_models = deployments
            .into_iter()
            .map(|s| GraphQlCostModel {
                deployment: s,
                price_per_byte: price,
            })
            .collect::<Vec<GraphQlCostModel>>();
        Ok(cost_models)
    }

    /// provide a cost model for a specific file/bundle served
    async fn cost_model(
        &self,
        ctx: &Context<'_>,
        deployment: String,
    ) -> Result<GraphQlCostModel, anyhow::Error> {
        let price: Option<f64> = ctx
            .data_unchecked::<ServerContext>()
            .state
            .prices
            .lock()
            .await
            .get(&deployment)
            .cloned();
        let res = GraphQlCostModel {
            deployment,
            price_per_byte: price.unwrap_or(
                ctx.data_unchecked::<ServerContext>()
                    .state
                    .config
                    .server
                    .default_price_per_byte,
            ),
        };
        Ok(res)
    }
}

pub type CostSchema = Schema<PriceQuery, EmptyMutation, EmptySubscription>;

pub async fn build_schema() -> CostSchema {
    Schema::build(PriceQuery, EmptyMutation, EmptySubscription).finish()
}

pub async fn cost(State(context): State<ServerContext>, req: GraphQLRequest) -> GraphQLResponse {
    context
        .state
        .cost_schema
        .execute(req.into_inner().data(context.clone()))
        .await
        .into()
}
