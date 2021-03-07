use std::result;

use tantivy::{query::QueryParserError, TantivyError};
use thiserror::Error;

mod index;
mod schema;
mod tokenizer;

pub use index::{DocType, Index, IndexDoc, QueryOptions};
pub use tantivy::tokenizer::Language;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Query error: {0}")]
    BadQuery(#[from] QueryParserError),
    #[error("Index error: {0}")]
    IndexError(#[from] TantivyError),
    #[error("Index is in an unhealthy state: {0}")]
    UnhealthyIndex(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}
