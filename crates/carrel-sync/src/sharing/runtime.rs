//! App-owned background exchange; Cozo remains authoritative while windows close.
use super::SocialBridge;
use carrel_core::identity::PublicKey;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{sync::Notify, task::JoinHandle};

/// Background mirror repair, receipt and bounded cache transfer.
pub struct SocialSyncWorker {
    /// Filtered service used by local commands.
    pub bridge: Arc<SocialBridge>,
    wake: Arc<Notify>,
    errors: Arc<std::sync::Mutex<BTreeMap<PublicKey, String>>>,
    task: JoinHandle<()>,
}
impl SocialSyncWorker {
    /// Continue exchange while Settings is closed, without notifications.
    pub fn start(bridge: Arc<SocialBridge>) -> Self {
        let wake = Arc::new(Notify::new());
        let signal = wake.clone();
        let errors = Arc::new(std::sync::Mutex::new(BTreeMap::new()));
        let failures = errors.clone();
        let background = bridge.clone();
        let task = tokio::spawn(async move {
            let mut publication = None;
            let mut cursors = BTreeMap::<PublicKey, Option<String>>::new();
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {_=tick.tick()=>{},_=signal.notified()=>{}}
                match background.publish(publication.take()).await {
                    Ok(after) => publication = after,
                    Err(error) => {
                        tracing::debug!(error_kind=?std::mem::discriminant(&error),"social mirror needs retry")
                    }
                }
                let Ok(contacts) = background.contacts() else {
                    continue;
                };
                let mut contacts = contacts.into_iter();
                let mut exchanges = tokio::task::JoinSet::new();
                loop {
                    while exchanges.len() < 2 {
                        let Some(card) = contacts.next() else {
                            break;
                        };
                        let master = card.certificate.master_pubkey;
                        let after = cursors.remove(&master).flatten();
                        let bridge = background.clone();
                        exchanges.spawn(async move {
                            match bridge.receive(&card, after).await {
                                Ok((after, _)) => {
                                    let failed = bridge
                                        .transfer_cache(&card)
                                        .await
                                        .map_or(true, |result| !result.errors.is_empty());
                                    (master, after, failed)
                                }
                                Err(_) => (master, None, true),
                            }
                        });
                    }
                    let Some(result) = exchanges.join_next().await else {
                        break;
                    };
                    let Ok((master, after, failed)) = result else {
                        continue;
                    };
                    cursors.insert(master, after);
                    let mut errors = failures.lock().expect("sharing status lock");
                    if failed {
                        errors.insert(master,"Could not finish the exchange. Keep both desktops open; saved articles remain available.".into());
                    } else {
                        errors.remove(&master);
                    }
                }
            }
        });
        Self {
            bridge,
            wake,
            errors,
            task,
        }
    }
    /// Retry deliberately after a local share or contact action.
    pub fn wake(&self) {
        self.wake.notify_one();
    }
    /// Source-specific recovery, shown only in People settings.
    pub fn error(&self, master: &PublicKey) -> Option<String> {
        self.errors
            .lock()
            .expect("sharing status lock")
            .get(master)
            .cloned()
    }
    /// Cancel network work before closing the private transport.
    pub fn stop(&self) {
        self.task.abort();
    }
}
impl Drop for SocialSyncWorker {
    fn drop(&mut self) {
        self.task.abort();
    }
}
