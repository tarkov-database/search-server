use tantivy::{
    tokenizer::{
        Language, LowerCaser, NgramTokenizer, RemoveLongFilter, SimpleTokenizer, Stemmer,
        StopWordFilter, TextAnalyzer,
    },
    Index,
};

const STOP_WORDS_OEC: [&str; 100] = [
    "the", "be", "to", "of", "and", "a", "in", "that", "have", "i", "it", "for", "not", "on",
    "with", "he", "as", "you", "do", "at", "this", "but", "his", "by", "from", "they", "we", "say",
    "her", "she", "or", "an", "will", "my", "one", "all", "would", "there", "their", "what", "so",
    "up", "out", "if", "about", "who", "get", "which", "go", "me", "when", "make", "can", "like",
    "time", "no", "just", "him", "know", "take", "people", "into", "year", "your", "good", "some",
    "could", "them", "see", "other", "than", "then", "now", "look", "only", "come", "its", "over",
    "think", "also", "back", "after", "use", "two", "how", "our", "work", "first", "well", "way",
    "even", "new", "want", "because", "any", "these", "give", "day", "most", "us",
];

#[derive(Debug)]
pub(crate) enum Tokenizer {
    Ngram(NgramOptions),
    Custom(Language),
}

impl Tokenizer {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Tokenizer::Ngram(_) => "ngram",
            Tokenizer::Custom(_) => "custom",
        }
    }

    pub(crate) fn register_for(self, index: &Index) {
        index.tokenizers().register(self.name(), self.to_analyzer());
    }

    pub(crate) fn to_analyzer(&self) -> TextAnalyzer {
        let stop_words = self.stop_words();

        match self {
            Tokenizer::Ngram(opts) => {
                TextAnalyzer::from(NgramTokenizer::new(opts.min, opts.max, opts.prefix))
                    .filter(LowerCaser)
                    .filter(stop_words)
            }
            Tokenizer::Custom(lang) => TextAnalyzer::from(SimpleTokenizer)
                .filter(RemoveLongFilter::limit(40))
                .filter(LowerCaser)
                .filter(stop_words)
                .filter(Stemmer::new(lang.to_owned())),
        }
    }

    fn stop_words(&self) -> StopWordFilter {
        let lang = match self {
            Tokenizer::Ngram(o) => &o.lang,
            Tokenizer::Custom(l) => l,
        };

        let stop_words = match lang {
            Language::English => STOP_WORDS_OEC.iter().map(|s| s.to_string()).collect(),
            _ => Vec::new(),
        };

        StopWordFilter::remove(stop_words)
    }
}

#[derive(Debug)]
pub(crate) struct NgramOptions {
    min: usize,
    max: usize,
    prefix: bool,
    lang: Language,
}

impl NgramOptions {
    pub(crate) fn new(min: usize, max: usize, prefix: bool) -> Self {
        Self {
            min,
            max,
            prefix,
            lang: Language::English,
        }
    }

    pub(crate) fn set_language(mut self, lang: Language) -> Self {
        self.lang = lang;
        self
    }
}

impl Default for NgramOptions {
    fn default() -> Self {
        Self::new(3, 4, false)
    }
}
