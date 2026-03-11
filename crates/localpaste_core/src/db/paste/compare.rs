//! Compare helpers for paste diff and equality operations.

use super::{deserialize_paste, PasteDb};
use crate::{
    db::{
        paste::helpers::deserialize_meta,
        tables::{PASTES, PASTES_META, PASTE_VERSIONS_META},
        versioning::decode_version_meta_list,
    },
    diff::{
        ensure_diff_input_within_limit, unified_diff_lines, DiffRef, DiffRequest, DiffResponse,
        EqualResponse,
    },
    error::AppError,
};
use redb::{ReadTransaction, ReadableDatabase};

enum ResolvedCompareInputs {
    SameRef,
    Different { left: String, right: String },
}

impl PasteDb {
    fn resolve_diff_ref_content_in_txn(
        &self,
        read_txn: &ReadTransaction,
        reference: &DiffRef,
    ) -> Result<Option<String>, AppError> {
        if let Some(version_id_ms) = reference.version_id_ms {
            return Ok(self
                .get_version_in_txn(read_txn, reference.paste_id.as_str(), version_id_ms)?
                .map(|snapshot| snapshot.content));
        }

        let pastes = read_txn.open_table(PASTES)?;
        match pastes.get(reference.paste_id.as_str())? {
            Some(value) => Ok(Some(deserialize_paste(value.value())?.content)),
            None => Ok(None),
        }
    }

    fn resolve_diff_ref_len_in_txn(
        &self,
        read_txn: &ReadTransaction,
        reference: &DiffRef,
    ) -> Result<Option<usize>, AppError> {
        if let Some(version_id_ms) = reference.version_id_ms {
            let versions_meta = read_txn.open_table(PASTE_VERSIONS_META)?;
            let Some(encoded) = versions_meta.get(reference.paste_id.as_str())? else {
                return Ok(None);
            };
            let version_items = decode_version_meta_list(Some(encoded.value()))?;
            return Ok(version_items
                .into_iter()
                .find(|item| item.version_id_ms == version_id_ms)
                .map(|item| item.len));
        }

        let metas = read_txn.open_table(PASTES_META)?;
        match metas.get(reference.paste_id.as_str())? {
            Some(value) => Ok(Some(deserialize_meta(value.value())?.content_len)),
            None => Ok(None),
        }
    }

    /// Resolve a [`DiffRef`] to raw content from head or a historical version.
    ///
    /// # Returns
    /// `Ok(Some(content))` when the reference resolves, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access fails.
    pub fn resolve_diff_ref_content(
        &self,
        reference: &DiffRef,
    ) -> Result<Option<String>, AppError> {
        let read_txn = self.db.begin_read()?;
        self.resolve_diff_ref_content_in_txn(&read_txn, reference)
    }

    /// Compute a diff within an existing read snapshot.
    ///
    /// # Arguments
    /// - `read_txn`: Shared snapshot used to resolve both compare refs.
    /// - `request`: Compare request describing the left and right refs.
    ///
    /// # Returns
    /// `Ok(Some(diff))` when both refs resolve, `Ok(None)` when either is missing.
    ///
    /// # Errors
    /// Returns an error when storage access or diff preparation fails.
    pub(super) fn diff_in_txn(
        &self,
        read_txn: &ReadTransaction,
        request: &DiffRequest,
    ) -> Result<Option<DiffResponse>, AppError> {
        let Some(compare_inputs) = self.resolve_compare_inputs_in_txn(read_txn, request)? else {
            return Ok(None);
        };
        let response = match compare_inputs {
            ResolvedCompareInputs::SameRef => DiffResponse {
                equal: true,
                unified: Vec::new(),
            },
            ResolvedCompareInputs::Different { left, right } => {
                let equal = left == right;
                let unified = if equal {
                    Vec::new()
                } else {
                    unified_diff_lines(left.as_str(), right.as_str())
                };
                DiffResponse { equal, unified }
            }
        };
        Ok(Some(response))
    }

    fn resolve_compare_inputs_in_txn(
        &self,
        read_txn: &ReadTransaction,
        request: &DiffRequest,
    ) -> Result<Option<ResolvedCompareInputs>, AppError> {
        if request.left == request.right {
            return Ok(self
                .resolve_diff_ref_content_in_txn(read_txn, &request.left)?
                .map(|_| ResolvedCompareInputs::SameRef));
        }

        let Some(left_len) = self.resolve_diff_ref_len_in_txn(read_txn, &request.left)? else {
            return Ok(None);
        };
        let Some(right_len) = self.resolve_diff_ref_len_in_txn(read_txn, &request.right)? else {
            return Ok(None);
        };
        ensure_diff_input_within_limit(left_len, right_len)?;

        let Some(left) = self.resolve_diff_ref_content_in_txn(read_txn, &request.left)? else {
            return Ok(None);
        };
        let Some(right) = self.resolve_diff_ref_content_in_txn(read_txn, &request.right)? else {
            return Ok(None);
        };
        Ok(Some(ResolvedCompareInputs::Different { left, right }))
    }

    /// Evaluate equality within an existing read snapshot.
    ///
    /// # Arguments
    /// - `read_txn`: Shared snapshot used to resolve both compare refs.
    /// - `request`: Compare request describing the left and right refs.
    ///
    /// # Returns
    /// `Ok(Some(equal))` when both refs resolve, `Ok(None)` when either is missing.
    ///
    /// # Errors
    /// Returns an error when storage access or compare preparation fails.
    pub(super) fn equal_in_txn(
        &self,
        read_txn: &ReadTransaction,
        request: &DiffRequest,
    ) -> Result<Option<EqualResponse>, AppError> {
        let Some(compare_inputs) = self.resolve_compare_inputs_in_txn(read_txn, request)? else {
            return Ok(None);
        };
        let response = match compare_inputs {
            ResolvedCompareInputs::SameRef => EqualResponse { equal: true },
            ResolvedCompareInputs::Different { left, right } => EqualResponse {
                equal: left == right,
            },
        };
        Ok(Some(response))
    }

    /// Compute a line-based diff between two paste references.
    ///
    /// # Returns
    /// `Ok(Some(diff))` when both references resolve, `Ok(None)` when either is missing.
    ///
    /// # Errors
    /// Returns an error when storage access fails.
    pub fn diff(&self, request: &DiffRequest) -> Result<Option<DiffResponse>, AppError> {
        // Both refs must resolve from one read snapshot so head-vs-head diffs do
        // not tear when a write lands between left and right lookups.
        let read_txn = self.db.begin_read()?;
        self.diff_in_txn(&read_txn, request)
    }

    /// Compare two paste references for equality without building a full diff.
    ///
    /// # Returns
    /// `Ok(Some(equal))` when both references resolve, `Ok(None)` when either is missing.
    ///
    /// # Errors
    /// Returns an error when storage access fails.
    pub fn equal(&self, request: &DiffRequest) -> Result<Option<EqualResponse>, AppError> {
        // Equality uses the same single-snapshot contract as diff so head-vs-head
        // comparisons cannot observe torn reads between left and right.
        let read_txn = self.db.begin_read()?;
        self.equal_in_txn(&read_txn, request)
    }
}
