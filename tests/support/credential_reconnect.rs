//! Real SSH credential reuse tests with a per-test store, never the user's credential entries.

#[cfg(unix)]
mod checks {
    use super::super::Fixture;
    use mantash::{
        credentials::{CredentialReply, PromptReason, SecretStore},
        events::Event,
        model::*,
        ssh,
        storage::Database,
    };
    use parking_lot::Mutex;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use zeroize::Zeroizing;

    #[derive(Default)]
    struct Store {
        value: Mutex<Option<Zeroizing<String>>>,
        reads: AtomicUsize,
        writes: AtomicUsize,
        deny_read: AtomicBool,
        deny_write: AtomicBool,
    }
    impl SecretStore for Store {
        fn read(&self, _: Id) -> anyhow::Result<Option<Zeroizing<String>>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            anyhow::ensure!(!self.deny_read.load(Ordering::SeqCst), "Test store locked");
            Ok(self.value.lock().clone())
        }
        fn write(&self, _: Id, secret: &str) -> anyhow::Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            anyhow::ensure!(
                !self.deny_write.load(Ordering::SeqCst),
                "Test store write denied"
            );
            *self.value.lock() = Some(Zeroizing::new(secret.to_owned()));
            Ok(())
        }
    }
    async fn event(events: &async_channel::Receiver<Event>) -> Event {
        tokio::time::timeout(Duration::from_secs(10), events.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn approve(events: &async_channel::Receiver<Event>) {
        loop {
            if let Event::HostKey { reply, .. } = event(events).await {
                reply.send(true).unwrap();
                return;
            }
        }
    }
    async fn prompt(
        events: &async_channel::Receiver<Event>,
        expected: PromptReason,
    ) -> (Owner, bool, tokio::sync::oneshot::Sender<CredentialReply>) {
        loop {
            if let Event::Credentials {
                owner,
                reason,
                remember,
                reply,
            } = event(events).await
            {
                assert_eq!(reason, expected);
                return (owner, remember, reply);
            }
        }
    }
    fn start(
        fixture: &Fixture,
        profile: Profile,
        db: Arc<Mutex<Database>>,
        store: Arc<Store>,
        cancel: CancellationToken,
    ) -> (
        tokio::task::JoinHandle<anyhow::Result<Arc<ssh::Remote>>>,
        async_channel::Receiver<Event>,
    ) {
        let (sender, receiver) = async_channel::unbounded();
        let task = tokio::spawn(ssh::connect_with_store(
            Owner::new(),
            profile,
            Zeroizing::new(String::new()),
            false,
            db,
            sender,
            cancel,
            store,
        ));
        let _ = fixture;
        (task, receiver)
    }
    async fn finish(
        task: tokio::task::JoinHandle<anyhow::Result<Arc<ssh::Remote>>>,
    ) -> Arc<ssh::Remote> {
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn saving_once_reuses_credentials_for_a_new_connection_and_does_not_rewrite_them() {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        let db = Arc::new(Mutex::new(Database::memory().unwrap()));
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            db.clone(),
            store.clone(),
            CancellationToken::new(),
        );
        loop {
            if let Event::HostKey { reply, .. } = event(&events).await {
                assert_eq!(store.reads.load(Ordering::SeqCst), 0);
                assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
                reply.send(true).unwrap();
                break;
            }
        }
        let (_, remember, reply) = prompt(&events, PromptReason::Missing).await;
        assert!(!remember);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
        assert!(
            reply
                .send(CredentialReply {
                    secret: Zeroizing::new(fixture.password.clone()),
                    remember: true
                })
                .is_ok()
        );
        let remote = finish(task).await;
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        remote.cancel.cancel();
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            db,
            store.clone(),
            CancellationToken::new(),
        );
        let remote = finish(task).await;
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        while let Ok(event) = events.try_recv() {
            assert!(!matches!(event, Event::Credentials { .. }));
        }
        remote.cancel.cancel();
    }

    #[tokio::test]
    async fn rejecting_a_changed_fingerprint_never_reads_or_sends_a_saved_password() {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        *store.value.lock() = Some(Zeroizing::new(fixture.password.clone()));
        let db = Arc::new(Mutex::new(Database::memory().unwrap()));
        db.lock()
            .trust(
                &fixture.profile.host,
                fixture.profile.port,
                "SHA256:previous",
            )
            .unwrap();
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            db,
            store.clone(),
            CancellationToken::new(),
        );
        loop {
            if let Event::HostKey {
                reply, previous, ..
            } = event(&events).await
            {
                assert_eq!(previous.as_deref(), Some("SHA256:previous"));
                reply.send(false).unwrap();
                break;
            }
        }
        assert!(task.await.unwrap().is_err());
        assert_eq!(store.reads.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn invalid_saved_password_waits_for_manual_correction_and_updates_only_after_success() {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        *store.value.lock() = Some(Zeroizing::new("wrong-test-password".into()));
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            Arc::new(Mutex::new(Database::memory().unwrap())),
            store.clone(),
            CancellationToken::new(),
        );
        approve(&events).await;
        let (_, remember, reply) = prompt(&events, PromptReason::Rejected).await;
        assert!(remember);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
        assert!(
            reply
                .send(CredentialReply {
                    secret: Zeroizing::new(fixture.password.clone()),
                    remember
                })
                .is_ok()
        );
        let remote = finish(task).await;
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        assert!(
            store
                .value
                .lock()
                .as_deref()
                .is_some_and(|p| p.as_str() == fixture.password)
        );
        remote.cancel.cancel();
    }

    #[tokio::test]
    async fn cancelling_a_password_prompt_invalidates_its_reply_and_never_saves() {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        let cancel = CancellationToken::new();
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            Arc::new(Mutex::new(Database::memory().unwrap())),
            store.clone(),
            cancel.clone(),
        );
        approve(&events).await;
        let (_, _, reply) = prompt(&events, PromptReason::Missing).await;
        cancel.cancel();
        assert!(task.await.unwrap().is_err());
        assert!(
            reply
                .send(CredentialReply {
                    secret: Zeroizing::new(fixture.password.clone()),
                    remember: true
                })
                .is_err()
        );
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_locked_store_can_use_manual_input_without_silently_saving() {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        store.deny_read.store(true, Ordering::SeqCst);
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            Arc::new(Mutex::new(Database::memory().unwrap())),
            store.clone(),
            CancellationToken::new(),
        );
        approve(&events).await;
        let (_, _, reply) = prompt(&events, PromptReason::StoreUnavailable).await;
        assert!(
            reply
                .send(CredentialReply {
                    secret: Zeroizing::new(fixture.password.clone()),
                    remember: false
                })
                .is_ok()
        );
        let remote = finish(task).await;
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
        remote.cancel.cancel();
    }

    #[tokio::test]
    async fn store_write_failure_reports_unsaved_while_the_authenticated_terminal_remains_available()
     {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        store.deny_write.store(true, Ordering::SeqCst);
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            Arc::new(Mutex::new(Database::memory().unwrap())),
            store.clone(),
            CancellationToken::new(),
        );
        approve(&events).await;
        let (_, _, reply) = prompt(&events, PromptReason::Missing).await;
        assert!(
            reply
                .send(CredentialReply {
                    secret: Zeroizing::new(fixture.password.clone()),
                    remember: true
                })
                .is_ok()
        );
        let remote = finish(task).await;
        let mut failed = false;
        while let Ok(event) = events.try_recv() {
            if matches!(event, Event::CredentialStorage { saved: false, .. }) {
                failed = true;
            }
        }
        assert!(failed);
        assert_eq!(
            remote
                .exec("printf CREDENTIAL_STORE_FAILURE_HANDLED")
                .await
                .unwrap(),
            "CREDENTIAL_STORE_FAILURE_HANDLED"
        );
        remote.cancel.cancel();
    }

    #[tokio::test]
    async fn unremembered_password_is_requested_again_on_the_next_connection() {
        let fixture = Fixture::new().await;
        let store = Arc::new(Store::default());
        let db = Arc::new(Mutex::new(Database::memory().unwrap()));
        let (task, events) = start(
            &fixture,
            fixture.profile.clone(),
            db.clone(),
            store.clone(),
            CancellationToken::new(),
        );
        approve(&events).await;
        let (_, _, reply) = prompt(&events, PromptReason::Missing).await;
        assert!(
            reply
                .send(CredentialReply {
                    secret: Zeroizing::new(fixture.password.clone()),
                    remember: false
                })
                .is_ok()
        );
        let remote = finish(task).await;
        remote.cancel.cancel();
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
        let cancel = CancellationToken::new();
        let (task, events) = start(&fixture, fixture.profile.clone(), db, store, cancel.clone());
        let (_, remember, reply) = prompt(&events, PromptReason::Missing).await;
        assert!(!remember);
        cancel.cancel();
        drop(reply);
        assert!(task.await.unwrap().is_err());
    }
}
