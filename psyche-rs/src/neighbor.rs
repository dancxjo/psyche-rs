use crate::memory_store::StoredImpression;

/// Merge two neighbor lists and deduplicate by impression id.
///
/// # Examples
/// ```
/// use psyche_rs::{merge_neighbors, StoredImpression};
/// use chrono::Utc;
///
/// let a = StoredImpression {
///     id: "1".into(),
///     kind: "Instant".into(),
///     when: Utc::now(),
///     how: "a".into(),
///     sensation_ids: Vec::new(),
///     impression_ids: Vec::new(),
/// };
/// let b = StoredImpression {
///     id: "1".into(),
///     kind: "Instant".into(),
///     when: Utc::now(),
///     how: "b".into(),
///     sensation_ids: Vec::new(),
///     impression_ids: Vec::new(),
/// };
/// let merged = merge_neighbors(vec![a], vec![b]);
/// assert_eq!(merged.len(), 1);
/// assert_eq!(merged[0].id, "1");
/// ```
pub fn merge_neighbors(
    mut a: Vec<StoredImpression>,
    mut b: Vec<StoredImpression>,
) -> Vec<StoredImpression> {
    a.append(&mut b);
    a.sort_by(|x, y| x.id.cmp(&y.id));
    a.dedup_by(|x, y| x.id == y.id);
    a
}
