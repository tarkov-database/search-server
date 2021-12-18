use crate::{
    extract::{Query, TokenData},
    model::Response,
    token::Claims,
};

use super::SearchError;

use axum::extract::Extension;
use search_index::{DocType, IndexDoc, QueryOptions};
use search_state::IndexState;
use serde::{Deserialize, Serialize};
use tracing::error;

const fn default_limit() -> usize {
    30
}

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    #[serde(alias = "q")]
    query: String,
    r#type: Option<DocType>,
    kind: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    conjunction: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    count: usize,
    data: Vec<IndexDoc>,
}

pub async fn get(
    TokenData(_claims): TokenData<Claims, true>,
    Query(opts): Query<QueryParams>,
    Extension(state): Extension<IndexState>,
) -> crate::Result<Response<SearchResult>> {
    let query = &opts.query;
    let options = QueryOptions {
        limit: opts.limit,
        conjunction: opts.conjunction,
    };

    match query.len() {
        l if l < 3 => return Err(SearchError::TermTooShort.into()),
        l if l > 100 => return Err(SearchError::TermTooLong.into()),
        _ => {}
    }

    let index = state.get_index();

    match if let Some(t) = opts.r#type {
        index.search_by_type(query, t, opts.kind.as_deref(), options)
    } else {
        index.query_top(query, options)
    } {
        Ok(d) => Ok(Response::new(SearchResult {
            count: d.len(),
            data: d,
        })),
        Err(e) => {
            error!("Error for query \"{}\": {}", query, e);
            Err(SearchError::IndexError(e).into())
        }
    }
}
