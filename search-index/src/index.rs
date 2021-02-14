use crate::{
    schema::{IndexField, IndexSchema},
    tokenizer::{NgramOptions, Tokenizer},
    Result,
};

use serde::Serialize;
use tantivy::{
    collector::TopDocs,
    query::{FuzzyTermQuery, QueryParser},
    schema::Schema,
    tokenizer::Language,
    Document, Index as TantivyIndex, IndexReader, ReloadPolicy, Term,
};
use tarkov_database_rs::model::item::Item;

const WRITE_BUFFER: usize = 50_000_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDoc {
    id: String,
    name: String,
    short_name: String,
    description: String,
    kind: String,
}

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
                &item.short_name,
            );
            doc.add_text(
                schema.get_field(IndexField::Name.name()).unwrap(),
                &item.name,
            );
            doc.add_text(
                schema
                    .get_field(IndexField::Description(self.lang).name())
                    .unwrap(),
                &item.description,
            );
            doc.add_text(
                schema.get_field(IndexField::Kind.name()).unwrap(),
                &item.kind,
            );

            writer.add_document(doc);
        }

        writer.commit()?;

        Ok(())
    }

    pub fn check_health(&self) -> Result<()> {
        if let Err(err) = self.index.validate_checksum() {
            return Err(crate::Error::UnhealthyIndex(format!(
                "Checksum error: {}",
                err
            )));
        }

        if self.index.searchable_segments()?.is_empty() {
            return Err(crate::Error::UnhealthyIndex(
                "No searchable segments".to_string(),
            ));
        }

        Ok(())
    }

    pub fn query_top(&self, query: &str, limit: usize) -> Result<Vec<ItemDoc>> {
        let id_field = self.schema.get_field(IndexField::ID.name()).unwrap();
        let name_field = self.schema.get_field(IndexField::Name.name()).unwrap();
        let desc_field = self
            .schema
            .get_field(IndexField::Description(self.lang).name())
            .unwrap();
        let kind_field = self.schema.get_field(IndexField::Kind.name()).unwrap();

        let collector = TopDocs::with_limit(limit);

        let mut parser = QueryParser::for_index(&self.index, vec![name_field, desc_field]);
        parser.set_field_boost(name_field, 2.0);

        let query = parser.parse_query(query)?;

        let searcher = self.reader.searcher();
        let docs = searcher.search(&query, &collector)?;

        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let mut result: Vec<ItemDoc> = Vec::with_capacity(docs.len());
        for (_, addr) in docs.into_iter() {
            let doc = searcher.doc(addr)?;
            let mut names = doc.get_all(name_field);
            let item = ItemDoc {
                id: doc.get_first(id_field).unwrap().text().unwrap().to_string(),
                short_name: names.next().unwrap().text().unwrap_or_default().to_string(),
                name: names.next().unwrap().text().unwrap_or_default().to_string(),
                description: doc
                    .get_first(desc_field)
                    .unwrap()
                    .text()
                    .unwrap_or_default()
                    .to_string(),
                kind: doc
                    .get_first(kind_field)
                    .unwrap()
                    .text()
                    .unwrap_or_default()
                    .to_string(),
            };

            result.push(item);
        }

        Ok(result)
    }

    pub fn query_top_fuzzy(&self, query: &str, limit: usize) -> Result<Vec<ItemDoc>> {
        let id_field = self.schema.get_field(IndexField::ID.name()).unwrap();
        let name_field = self.schema.get_field(IndexField::Name.name()).unwrap();
        let desc_field = self
            .schema
            .get_field(IndexField::Description(self.lang).name())
            .unwrap();
        let kind_field = self.schema.get_field(IndexField::Kind.name()).unwrap();

        let collector = TopDocs::with_limit(limit);

        let term = Term::from_field_text(name_field, query);
        let query = FuzzyTermQuery::new(term, 1, true);

        let searcher = self.reader.searcher();
        let docs = searcher.search(&query, &collector)?;

        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let mut result: Vec<ItemDoc> = Vec::with_capacity(docs.len());
        for (_, addr) in docs.into_iter() {
            let doc = searcher.doc(addr)?;
            let mut names = doc.get_all(name_field);
            let item = ItemDoc {
                id: doc.get_first(id_field).unwrap().text().unwrap().to_string(),
                short_name: names.next().unwrap().text().unwrap_or_default().to_string(),
                name: names.next().unwrap().text().unwrap_or_default().to_string(),
                description: doc
                    .get_first(desc_field)
                    .unwrap()
                    .text()
                    .unwrap_or_default()
                    .to_string(),
                kind: doc
                    .get_first(kind_field)
                    .unwrap()
                    .text()
                    .unwrap_or_default()
                    .to_string(),
            };

            result.push(item);
        }

        Ok(result)
    }
}
