use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{RemoteObject, Result, StorageAdapter, StorageError, TargetId};

/// `None` fields disable that rule. The newest backup is never selected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    pub keep_last: Option<u32>,
    pub max_age_days: Option<u32>,
}

impl RetentionPolicy {
    pub fn is_disabled(&self) -> bool {
        self.keep_last.is_none() && self.max_age_days.is_none()
    }
}

/// Objects that exceed the policy (pure function, input in any order).
///
/// Objects are ranked newest first (objects without `created_at` rank last, in
/// input order). `keep_last` selects everything past the first N (N is at least 1);
/// `max_age_days` selects dated objects older than the cutoff. The newest object is
/// never selected. The result keeps the newest-first ranking.
pub fn select_expired<'a>(
    objects: &'a [RemoteObject],
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
) -> Vec<&'a RemoteObject> {
    if policy.is_disabled() || objects.len() < 2 {
        return Vec::new();
    }

    let mut ranked: Vec<&RemoteObject> = objects.iter().collect();
    // Stable sort: newest first, undated objects last in their original order.
    ranked.sort_by(|a, b| match (a.created_at, b.created_at) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });

    let keep_last = policy.keep_last.map(|n| n.max(1) as usize);
    let cutoff = policy.max_age_days.map(|days| now - Duration::days(i64::from(days)));

    ranked
        .into_iter()
        .enumerate()
        .skip(1)
        .filter(|(rank, object)| {
            let over_count = keep_last.is_some_and(|n| *rank >= n);
            let too_old = matches!((cutoff, object.created_at), (Some(cutoff), Some(created)) if created < cutoff);
            over_count || too_old
        })
        .map(|(_, object)| object)
        .collect()
}

/// Lists, selects and deletes expired backups. Returns the deleted objects.
///
/// Objects that disappeared in the meantime (`NotFound`) count as deleted; any other
/// error stops the run and is returned.
pub async fn apply_retention(
    adapter: &dyn StorageAdapter,
    target: &TargetId,
    policy: &RetentionPolicy,
) -> Result<Vec<RemoteObject>> {
    if policy.is_disabled() {
        return Ok(Vec::new());
    }
    let objects = adapter.list_backups(target).await?;
    let expired: Vec<RemoteObject> = select_expired(&objects, policy, Utc::now()).into_iter().cloned().collect();

    let mut deleted = Vec::with_capacity(expired.len());
    for object in expired {
        match adapter.delete(&object).await {
            Ok(()) | Err(StorageError::NotFound(_)) => {
                tracing::info!(provider = adapter.provider_id(), name = %object.name, "retention deleted backup");
                deleted.push(object);
            }
            Err(err) => return Err(err),
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn obj(id: &str, day: Option<u32>) -> RemoteObject {
        RemoteObject {
            id: id.into(),
            name: format!("{id}.zip"),
            size: Some(1),
            created_at: day.map(|d| Utc.with_ymd_and_hms(2026, 9, d, 12, 0, 0).unwrap()),
            instance_id: None,
            web_link: None,
        }
    }

    fn ids(selected: Vec<&RemoteObject>) -> Vec<&str> {
        selected.into_iter().map(|o| o.id.as_str()).collect()
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 16, 12, 0, 0).unwrap()
    }

    #[test]
    fn disabled_policy_selects_nothing() {
        let objects = vec![obj("a", Some(1)), obj("b", Some(2))];
        assert!(select_expired(&objects, &RetentionPolicy::default(), now()).is_empty());
    }

    #[test]
    fn keep_last_keeps_newest_regardless_of_input_order() {
        let objects =
            vec![obj("d2", Some(2)), obj("d5", Some(5)), obj("d1", Some(1)), obj("d4", Some(4)), obj("d3", Some(3))];
        let policy = RetentionPolicy { keep_last: Some(2), max_age_days: None };
        assert_eq!(ids(select_expired(&objects, &policy, now())), vec!["d3", "d2", "d1"]);
    }

    #[test]
    fn keep_last_zero_still_keeps_the_newest() {
        let objects = vec![obj("a", Some(1)), obj("b", Some(2))];
        let policy = RetentionPolicy { keep_last: Some(0), max_age_days: None };
        assert_eq!(ids(select_expired(&objects, &policy, now())), vec!["a"]);
    }

    #[test]
    fn max_age_selects_old_but_never_the_newest() {
        let objects = vec![obj("old1", Some(1)), obj("old2", Some(2)), obj("recent", Some(15))];
        let policy = RetentionPolicy { keep_last: None, max_age_days: Some(7) };
        assert_eq!(ids(select_expired(&objects, &policy, now())), vec!["old2", "old1"]);

        let all_old = vec![obj("old1", Some(1)), obj("old2", Some(2))];
        assert_eq!(ids(select_expired(&all_old, &policy, now())), vec!["old1"]);
    }

    #[test]
    fn undated_objects_are_never_selected_by_age() {
        let objects = vec![obj("new", Some(15)), obj("undated", None), obj("old", Some(1))];
        let policy = RetentionPolicy { keep_last: None, max_age_days: Some(7) };
        assert_eq!(ids(select_expired(&objects, &policy, now())), vec!["old"]);

        let by_count = RetentionPolicy { keep_last: Some(1), max_age_days: None };
        assert_eq!(ids(select_expired(&objects, &by_count, now())), vec!["old", "undated"]);
    }

    #[test]
    fn rules_combine() {
        let objects = vec![obj("d15", Some(15)), obj("d14", Some(14)), obj("d13", Some(13)), obj("d2", Some(2))];
        let policy = RetentionPolicy { keep_last: Some(3), max_age_days: Some(10) };
        assert_eq!(ids(select_expired(&objects, &policy, now())), vec!["d2"]);
        let policy = RetentionPolicy { keep_last: Some(2), max_age_days: Some(10) };
        assert_eq!(ids(select_expired(&objects, &policy, now())), vec!["d13", "d2"]);
    }
}
