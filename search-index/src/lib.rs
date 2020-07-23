use serde::Serialize;
use tantivy::{
    collector::TopDocs,
    query::{FuzzyTermQuery, QueryParser},
    schema::{IndexRecordOption, Schema, SchemaBuilder, TextFieldIndexing, TextOptions},
    tokenizer::{
        Language, LowerCaser, NgramTokenizer, RemoveLongFilter, SimpleTokenizer, Stemmer,
        TextAnalyzer,
    },
    Document, Index, IndexReader, ReloadPolicy, Term,
};
use tarkov_database_rs::model::item::Item;

pub use tantivy::TantivyError as Error;

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

pub struct ItemIndex {
    index: Index,
    reader: IndexReader,
    schema: Schema,
}

impl ItemIndex {
    pub fn new() -> tantivy::Result<Self> {
        let mut builder = SchemaBuilder::default();

        let id = TextOptions::default().set_stored();
        builder.add_text_field("id", id);

        let name = TextOptions::default().set_stored().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("ngram")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        builder.add_text_field("name", name);

        let description = TextOptions::default().set_stored().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("custom_en")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        builder.add_text_field("description", description);

        let kind = TextOptions::default().set_stored().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::Basic),
        );
        builder.add_text_field("kind", kind);

        let schema = builder.build();

        let index = Index::create_from_tempdir(schema.clone())?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommit)
            .try_into()?;

        let en_stem = TextAnalyzer::from(SimpleTokenizer)
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .filter(Stemmer::new(Language::English));
        index.tokenizers().register("custom_en", en_stem);

        let ngram = TextAnalyzer::from(NgramTokenizer::new(3, 4, false)).filter(LowerCaser);
        index.tokenizers().register("ngram", ngram);

        Ok(Self {
            index,
            reader,
            schema,
        })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn write_index(&self, data: Vec<Item>) -> tantivy::Result<()> {
        let mut writer = self.index.writer(WRITE_BUFFER)?;
        let schema = &self.schema;

        // TODO: Make it more intelligent
        writer.delete_all_documents()?;

        for item in data.into_iter() {
            let mut doc = Document::default();
            doc.add_text(schema.get_field("id").unwrap(), &item.id);
            doc.add_text(schema.get_field("name").unwrap(), &item.short_name);
            doc.add_text(schema.get_field("name").unwrap(), &item.name);
            doc.add_text(schema.get_field("description").unwrap(), &item.description);
            doc.add_text(schema.get_field("kind").unwrap(), &item.kind);

            writer.add_document(doc);
        }

        writer.commit()?;

        Ok(())
    }

    pub fn query_top(&self, term: &str, limit: usize) -> tantivy::Result<Vec<ItemDoc>> {
        let id_field = self.schema.get_field("id").unwrap();
        let name_field = self.schema.get_field("name").unwrap();
        let desc_field = self.schema.get_field("description").unwrap();
        let kind_field = self.schema.get_field("kind").unwrap();

        let collector = TopDocs::with_limit(limit);

        let mut parser = QueryParser::for_index(&self.index, vec![name_field, desc_field]);
        parser.set_field_boost(name_field, 2.0);

        let query = parser.parse_query(term)?;

        let searcher = self.reader.searcher();
        let docs = searcher.search(&query, &collector)?;

        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let mut result: Vec<ItemDoc> = Vec::with_capacity(docs.len());
        for (_, addr) in docs.into_iter() {
            let doc = searcher.doc(addr)?;
            let names = doc.get_all(name_field);
            let item = ItemDoc {
                id: doc.get_first(id_field).unwrap().text().unwrap().to_string(),
                name: names[1].text().unwrap_or_default().to_string(),
                short_name: names[0].text().unwrap_or_default().to_string(),
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

    pub fn query_top_fuzzy(&self, term: &str, limit: usize) -> tantivy::Result<Vec<ItemDoc>> {
        let id_field = self.schema.get_field("id").unwrap();
        let name_field = self.schema.get_field("name").unwrap();
        let desc_field = self.schema.get_field("description").unwrap();
        let kind_field = self.schema.get_field("kind").unwrap();

        let collector = TopDocs::with_limit(limit);

        let term = Term::from_field_text(name_field, term);
        let query = FuzzyTermQuery::new(term, 1, true);

        let searcher = self.reader.searcher();
        let docs = searcher.search(&query, &collector)?;

        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let mut result: Vec<ItemDoc> = Vec::with_capacity(docs.len());
        for (_, addr) in docs.into_iter() {
            let doc = searcher.doc(addr)?;
            let names = doc.get_all(name_field);
            let item = ItemDoc {
                id: doc.get_first(id_field).unwrap().text().unwrap().to_string(),
                name: names[1].text().unwrap_or_default().to_string(),
                short_name: names[0].text().unwrap_or_default().to_string(),
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
