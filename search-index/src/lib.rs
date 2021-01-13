use std::result;

use tantivy::{query::QueryParserError, TantivyError};
use thiserror::Error;

mod index;
mod schema;
mod tokenizer;

pub use index::{Index, ItemDoc};
pub use tantivy::tokenizer::Language;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Query error: {0}")]
    BadQuery(#[from] QueryParserError),
    #[error("Index error: {0}")]
    IndexError(#[from] TantivyError),
}
