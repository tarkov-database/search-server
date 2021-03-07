use crate::tokenizer::{NgramOptions, Tokenizer};

use tantivy::{
    schema::{
        FieldEntry, IndexRecordOption, Schema, SchemaBuilder, TextFieldIndexing, TextOptions,
    },
    tokenizer::Language,
};

#[derive(Debug)]
pub(crate) enum IndexField {
    ID,
    Name,
    Description(Language),
    Kind,
    Type,
}

impl IndexField {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            IndexField::ID => "id",
            IndexField::Name => "name",
            IndexField::Description(_) => "description",
            IndexField::Kind => "kind",
            IndexField::Type => "type",
        }
    }

    fn options(&self) -> Option<TextOptions> {
        match self {
            IndexField::ID => Some(TextOptions::default().set_stored()),
            IndexField::Name => Some(
                TextOptions::default().set_stored().set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer(Tokenizer::Ngram(NgramOptions::default()).name())
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                ),
            ),
            IndexField::Description(lang) => Some(
                TextOptions::default().set_stored().set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer(Tokenizer::Custom(lang.to_owned()).name())
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                ),
            ),
            IndexField::Kind => Some(
                TextOptions::default().set_stored().set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("default")
                        .set_index_option(IndexRecordOption::Basic),
                ),
            ),
            IndexField::Type => Some(
                TextOptions::default().set_stored().set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("default")
                        .set_index_option(IndexRecordOption::Basic),
                ),
            ),
        }
    }
}

impl ToString for IndexField {
    fn to_string(&self) -> String {
        self.name().to_string()
    }
}

impl Into<String> for IndexField {
    fn into(self) -> String {
        self.to_string()
    }
}

impl Into<FieldEntry> for IndexField {
    fn into(self) -> FieldEntry {
        match self {
            IndexField::ID
            | IndexField::Name
            | IndexField::Description(_)
            | IndexField::Kind
            | IndexField::Type => {
                let name = self.to_string();
                let opts = match self.options() {
                    Some(o) => o,
                    None => TextOptions::default(),
                };

                FieldEntry::new_text(name, opts)
            }
        }
    }
}

pub(crate) struct IndexSchema {
    lang: Language,
}

impl IndexSchema {
    pub(crate) fn with_lang(lang: Language) -> Self {
        Self { lang }
    }

    pub(crate) fn build(self) -> Schema {
        let mut builder = SchemaBuilder::default();

        builder.add_field(IndexField::ID.into());
        builder.add_field(IndexField::Name.into());
        builder.add_field(IndexField::Description(self.lang).into());
        builder.add_field(IndexField::Kind.into());
        builder.add_field(IndexField::Type.into());

        builder.build()
    }
}

impl Default for IndexSchema {
    fn default() -> Self {
        Self::with_lang(Language::English)
    }
}
