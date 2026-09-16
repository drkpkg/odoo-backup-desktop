//! Registry of running backup jobs and a concurrency limiter whose limit can
//! change at runtime (settings).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStage {
    Requesting,
    ServerPreparing,
    Downloading,
    Validating,
    Uploading,
    Retention,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveJob {
    pub job_id: String,
    pub instance_id: String,
    pub stage: BackupStage,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent: Option<u64>,
}

struct JobHandle {
    snapshot: ActiveJob,
    cancel: CancellationToken,
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    jobs: Arc<Mutex<HashMap<String, JobHandle>>>,
}

impl JobRegistry {
    pub fn register(&self, job_id: &str, instance_id: &str, cancel: CancellationToken) {
        let snapshot = ActiveJob {
            job_id: job_id.to_owned(),
            instance_id: instance_id.to_owned(),
            stage: BackupStage::Requesting,
            started_at: Utc::now(),
            elapsed_secs: None,
            received: None,
            total: None,
            sent: None,
        };
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.insert(job_id.to_owned(), JobHandle { snapshot, cancel });
        }
    }

    pub fn update(&self, job_id: &str, f: impl FnOnce(&mut ActiveJob)) {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(handle) = jobs.get_mut(job_id)
        {
            f(&mut handle.snapshot);
        }
    }

    pub fn remove(&self, job_id: &str) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(job_id);
        }
    }

    pub fn cancel(&self, job_id: &str) -> bool {
        self.jobs.lock().ok().and_then(|jobs| jobs.get(job_id).map(|handle| handle.cancel.cancel())).is_some()
    }

    pub fn is_instance_running(&self, instance_id: &str) -> bool {
        self.jobs.lock().map(|jobs| jobs.values().any(|h| h.snapshot.instance_id == instance_id)).unwrap_or(false)
    }

    pub fn snapshots(&self) -> Vec<ActiveJob> {
        let mut list: Vec<ActiveJob> =
            self.jobs.lock().map(|jobs| jobs.values().map(|h| h.snapshot.clone()).collect()).unwrap_or_default();
        list.sort_by_key(|job| job.started_at);
        list
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.lock().map(|jobs| jobs.is_empty()).unwrap_or(true)
    }
}

/// Semaphore-like limiter that reads its limit on every acquisition.
#[derive(Clone)]
pub struct Limiter {
    running: Arc<Mutex<usize>>,
    notify: Arc<Notify>,
}

pub struct Permit {
    limiter: Limiter,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut running) = self.limiter.running.lock() {
            *running = running.saturating_sub(1);
        }
        self.limiter.notify.notify_waiters();
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self { running: Arc::new(Mutex::new(0)), notify: Arc::new(Notify::new()) }
    }
}

impl Limiter {
    /// Waits for a free slot (`limit` is re-read after every wake-up) or cancellation.
    pub async fn acquire(&self, limit: impl Fn() -> usize, cancel: &CancellationToken) -> Option<Permit> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut running = self.running.lock().ok()?;
                if *running < limit().max(1) {
                    *running += 1;
                    return Some(Permit { limiter: self.clone() });
                }
            }
            tokio::select! {
                _ = &mut notified => {}
                _ = cancel.cancelled() => return None,
                // Re-check periodically in case the limit was raised.
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limiter_respects_limit_and_releases() {
        let limiter = Limiter::default();
        let cancel = CancellationToken::new();
        let first = limiter.acquire(|| 1, &cancel).await.unwrap();

        let limiter2 = limiter.clone();
        let cancel2 = cancel.clone();
        let waiter = tokio::spawn(async move { limiter2.acquire(|| 1, &cancel2).await.is_some() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());

        drop(first);
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn limiter_wait_is_cancellable() {
        let limiter = Limiter::default();
        let cancel = CancellationToken::new();
        let _held = limiter.acquire(|| 1, &cancel).await.unwrap();
        let other = CancellationToken::new();
        other.cancel();
        assert!(limiter.acquire(|| 1, &other).await.is_none());
    }

    #[test]
    fn registry_tracks_jobs() {
        let registry = JobRegistry::default();
        let token = CancellationToken::new();
        registry.register("j1", "i1", token.clone());
        assert!(registry.is_instance_running("i1"));
        registry.update("j1", |job| job.stage = BackupStage::Downloading);
        assert_eq!(registry.snapshots()[0].stage, BackupStage::Downloading);
        assert!(registry.cancel("j1"));
        assert!(token.is_cancelled());
        registry.remove("j1");
        assert!(registry.is_empty());
    }
}
