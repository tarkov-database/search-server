use crate::{
    schema::{IndexField, IndexSchema},
    tokenizer::{NgramOptions, Tokenizer},
    Error, Result,
};

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use tantivy::{
    collector::TopDocs, query::QueryParser, schema::Schema, tokenizer::Language, Document,
    Index as TantivyIndex, IndexReader, ReloadPolicy,
};
use tarkov_database_rs::model::item::common::Item;

const WRITE_BUFFER: usize = 50_000_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDoc {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    short_name: Option<String>,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    r#type: DocType,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub enum DocType {
    Item,
    Location,
    Module,
}

impl FromStr for DocType {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self> {
        let t = match s {
            "item" => DocType::Item,
            "location" => DocType::Location,
            "module" => DocType::Module,
            _ => return Err(Error::ParseError("unknown doc type".to_string())),
        };

        Ok(t)
    }
}

impl fmt::Display for DocType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocType::Item => write!(f, "item"),
            DocType::Location => write!(f, "location"),
            DocType::Module => write!(f, "module"),
        }
    }
}

#[derive(Debug)]
pub struct QueryOptions {
    pub limit: usize,
    pub conjunction: bool,
}

#[derive(Clone)]
pub struct Index {
    index: TantivyIndex,
    reader: IndexReader,
    schema: Schema,
    lang: Language,
}

impl Index {
    pub fn new() -> Result<Self> {
        Self::with_lang(Language::English)
    }

    pub fn with_lang(lang: Language) -> Result<Self> {
        let schema = IndexSchema::with_lang(lang).build();

        let index = TantivyIndex::create_from_tempdir(schema.clone())?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into()?;

        let custom = Tokenizer::Custom(lang);
        custom.register_for(&index);

        let ngram = Tokenizer::Ngram(NgramOptions::default().set_language(lang));
        ngram.register_for(&index);

        Ok(Self {
            index,
            reader,
            schema,
            lang,
        })
    }

    pub fn write_index(&self, data: Vec<Item>) -> Result<()> {
        let mut writer = self.index.writer(WRITE_BUFFER)?;
        let schema = &self.schema;

        // TODO: Make it more intelligent
        writer.delete_all_documents()?;

        for item in data.into_iter() {
            let mut doc = Document::default();
            doc.add_text(schema.get_field(IndexField::ID.name()).unwrap(), &item.id);
            doc.add_text(
                schema.get_field(IndexField::Name.name()).unwrap(),
                item.short_name,
            );
            doc.add_text(
                schema.get_field(IndexField::Name.name()).unwrap(),
                item.name,
            );
            doc.add_text(
                schema
                    .get_field(IndexField::Description(self.lang).name())
                    .unwrap(),
                item.description,
            );
            doc.add_text(
                schema.get_field(IndexField::Kind.name()).unwrap(),
                item.kind,
            );
            doc.add_text(
                schema.get_field(IndexField::Type.name()).unwrap(),
                DocType::Item.to_string(),
            );

            writer.add_document(doc)?;
        }

        writer.commit()?;

        Ok(())
    }

    pub fn check_health(&self) -> Result<()> {
        if let Err(err) = self.index.validate_checksum() {
            return Err(Error::UnhealthyIndex(format!("Checksum error: {}", err)));
        }

        if self.index.searchable_segments()?.is_empty() {
            return Err(Error::UnhealthyIndex("No searchable segments".to_string()));
        }

        Ok(())
    }

    // Replace with query builder?
    pub fn search_by_type(
        &self,
        query: &str,
        r#type: DocType,
        kind: Option<&[&str]>,
        opts: QueryOptions,
    ) -> Result<Vec<IndexDoc>> {
        let mut q = format!("type:{}", r#type);

        if r#type == DocType::Item {
            if let Some(k) = kind {
                let len = k.len();
                let k = k
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if i == len - 1 {
                            format!("kind:{}", v)
                        } else {
                            format!("kind:{} OR ", v)
                        }
                    })
                    .collect::<Vec<_>>()
                    .concat();
                q = format!("{} AND ({})", q, k);
            }
        }

        self.query_top(&format!("{} AND ({})", q, query), opts)
    }

    pub fn query_top(&self, query: &str, opts: QueryOptions) -> Result<Vec<IndexDoc>> {
        let id_field = self.schema.get_field(IndexField::ID.name()).unwrap();
        let name_field = self.schema.get_field(IndexField::Name.name()).unwrap();
        let desc_field = self
            .schema
            .get_field(IndexField::Description(self.lang).name())
            .unwrap();
        let kind_field = self.schema.get_field(IndexField::Kind.name()).unwrap();
        let type_field = self.schema.get_field(IndexField::Type.name()).unwrap();

        let collector = TopDocs::with_limit(opts.limit);

        let mut parser = QueryParser::for_index(&self.index, vec![name_field, desc_field]);
        parser.set_field_boost(name_field, 2.0);

        if opts.conjunction {
            parser.set_conjunction_by_default();
        }

        let query = parser.parse_query(query)?;

        let searcher = self.reader.searcher();
        let docs = searcher.search(&query, &collector)?;

        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let mut result: Vec<IndexDoc> = Vec::with_capacity(docs.len());
        for (_, addr) in docs.into_iter() {
            let doc = searcher.doc(addr)?;
            let mut names = doc.get_all(name_field);
            let mut item = IndexDoc {
                id: doc
                    .get_first(id_field)
                    .unwrap()
                    .as_text()
                    .unwrap()
                    .to_string(),
                short_name: None,
                name: String::new(),
                description: doc
                    .get_first(desc_field)
                    .unwrap()
                    .as_text()
                    .unwrap_or_default()
                    .to_string(),
                kind: None,
                r#type: DocType::from_str(
                    doc.get_first(type_field)
                        .unwrap()
                        .as_text()
                        .unwrap_or_default(),
                )
                .unwrap(),
            };

            if item.r#type == DocType::Item {
                item.short_name = Some(names.next().unwrap().as_text().unwrap().to_string());
            }

            item.name.push_str(names.next().unwrap().as_text().unwrap());

            item.kind = doc
                .get_first(kind_field)
                .unwrap()
                .as_text()
                .map(|s| s.to_string());

            result.push(item);
        }

        Ok(result)
    }
}
