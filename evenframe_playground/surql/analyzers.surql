-- Analyzers used by #[fulltext(analyzer = "...")] field attributes.
-- Applied by schemasync before tables; keep OVERWRITE so re-syncs succeed.

DEFINE ANALYZER OVERWRITE blog_english
    TOKENIZERS blank, class
    FILTERS lowercase, ascii, snowball(english)
    COMMENT 'English stemming for blog post search';
