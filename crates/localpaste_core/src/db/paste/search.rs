//! Scoped search over canonical bodies or the metadata projection.

use super::*;

impl PasteDb {
    /// Search canonical paste data and return ranked metadata rows.
    ///
    /// # Arguments
    /// - `query`: Search query string.
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    ///
    /// # Returns
    /// Ranked metadata matches (name/tags/content scoring).
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        self.search_with_options(query, limit, folder_id, language, SearchOptions::default())
    }

    /// Search canonical paste data with explicit search behavior flags.
    ///
    /// # Arguments
    /// - `query`: Search query string.
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    /// - `options`: Search behavior flags.
    ///
    /// # Returns
    /// Ranked metadata matches (name/tags/content scoring).
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn search_with_options(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
        options: SearchOptions,
    ) -> Result<Vec<PasteMeta>, AppError> {
        self.search_scoped_with_options(
            query,
            limit,
            folder_id,
            language,
            options,
            SearchScope::All,
        )
    }

    /// Search metadata-only fields and return ranked rows.
    ///
    /// # Arguments
    /// - `query`: Search query string.
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    ///
    /// # Returns
    /// Ranked metadata matches (name/tags/language scoring).
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn search_meta(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        self.search_meta_with_options(query, limit, folder_id, language, SearchOptions::default())
    }

    /// Search metadata-only fields with explicit search behavior flags.
    ///
    /// # Arguments
    /// - `query`: Search query string.
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    /// - `options`: Search behavior flags.
    ///
    /// # Returns
    /// Ranked metadata matches (name/tags/language scoring).
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn search_meta_with_options(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
        options: SearchOptions,
    ) -> Result<Vec<PasteMeta>, AppError> {
        self.search_scoped_with_options(
            query,
            limit,
            folder_id,
            language,
            options,
            SearchScope::Metadata,
        )
    }
    /// Search the full store within an explicit field scope.
    ///
    /// Title and Metadata read only the derived metadata table. All and Body read
    /// canonical bodies; filters and top-k ranking apply before the result limit.
    ///
    /// # Arguments
    /// - `query`: Search text, trimmed before matching.
    /// - `limit`: Maximum number of results.
    /// - `folder_id`: Optional folder restriction.
    /// - `language`: Optional canonical language restriction.
    /// - `options`: Case-sensitivity policy.
    /// - `scope`: Fields eligible to match.
    ///
    /// # Returns
    /// Ranked metadata rows from the complete store.
    ///
    /// # Errors
    /// Returns storage or row-deserialization errors.
    pub fn search_scoped_with_options(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
        options: SearchOptions,
        scope: SearchScope,
    ) -> Result<Vec<PasteMeta>, AppError> {
        self.search_scoped_filtered_with_options(
            query,
            limit,
            folder_id,
            language,
            options,
            scope,
            |_| true,
        )
    }

    /// Search the full store within an explicit field scope and caller filter.
    ///
    /// The caller filter runs against persisted metadata before canonical bodies
    /// are loaded or ranked. This lets smart collections constrain a full-store
    /// search without losing matches beyond an unfiltered result limit.
    ///
    /// # Arguments
    /// - `query`: Search text, trimmed before matching.
    /// - `limit`: Maximum number of results.
    /// - `folder_id`: Optional folder restriction.
    /// - `language`: Optional canonical language restriction.
    /// - `options`: Case-sensitivity policy.
    /// - `scope`: Fields eligible to match.
    /// - `matches`: Additional metadata predicate for each candidate.
    ///
    /// # Returns
    /// Ranked metadata rows from the complete store that satisfy all filters.
    ///
    /// # Errors
    /// Returns storage or row-deserialization errors.
    pub fn search_scoped_filtered_with_options<F>(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
        options: SearchOptions,
        scope: SearchScope,
        matches: F,
    ) -> Result<Vec<PasteMeta>, AppError>
    where
        F: Fn(&PasteMeta) -> bool,
    {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let language_filter = normalize_language_filter(language.as_deref());
        let metadata_only = matches!(scope, SearchScope::Title | SearchScope::Metadata);
        let read_txn = self.db.begin_read()?;
        let metas = read_txn.open_table(PASTES_META)?;
        let query_lower = query.to_lowercase();
        let literal_query = if options.case_sensitive {
            query
        } else {
            &query_lower
        };
        let mut results = Vec::new();
        if metadata_only {
            for row in metas.iter()? {
                let (_, value) = row?;
                let meta = deserialize_meta(value.value())?;
                if !meta_matches_filters(&meta, folder_id.as_deref(), language_filter.as_deref())
                    || !matches(&meta)
                {
                    continue;
                }
                let score = if scope == SearchScope::Title {
                    i32::from(helpers::contains_search(
                        &meta.name,
                        literal_query,
                        options.case_sensitive,
                    )) * 12
                } else {
                    score_meta_match(&meta, query, options.case_sensitive)
                };
                if score > 0 {
                    push_ranked_meta_top_k(&mut results, (score, meta.updated_at, meta), limit);
                }
            }
        } else {
            let pastes = read_txn.open_table(PASTES)?;
            for row in metas.iter()? {
                let (key, value) = row?;
                let meta = deserialize_meta(value.value())?;
                if !meta_matches_filters(&meta, folder_id.as_deref(), language_filter.as_deref())
                    || !matches(&meta)
                {
                    continue;
                }
                let Some(value) = pastes.get(key.value())? else {
                    continue;
                };
                let paste = deserialize_paste(value.value())?;
                let score = if scope == SearchScope::Body {
                    i32::from(helpers::contains_search(
                        &paste.content,
                        literal_query,
                        options.case_sensitive,
                    ))
                } else {
                    score_paste_match(&paste, &meta, query, options.case_sensitive)
                };
                if score > 0 {
                    push_ranked_meta_top_k(&mut results, (score, meta.updated_at, meta), limit);
                }
            }
        }
        Ok(finalize_meta_search_results(results, limit))
    }
}
