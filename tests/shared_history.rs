//! Stable shared SSH history and frozen visible selection, independent of host metadata.
use mantash::{model::*, storage::Database};
use std::collections::HashSet;

#[test]
fn stable_identities_and_membership_survive_reload_and_delete() {
    let directory = tempfile::tempdir().unwrap();
    let mut db = Database::open(directory.path()).unwrap();
    let first = db
        .record_history(HistoryScope::Ssh, " git status ")
        .unwrap()
        .items[0]
        .id;
    let second = db
        .record_history(HistoryScope::Ssh, "kubectl get pods")
        .unwrap()
        .items[0]
        .id;
    let local = db
        .record_history(HistoryScope::Local, "git status")
        .unwrap()
        .items[0]
        .id;
    assert_ne!(first, local);
    assert_eq!(
        db.record_history(HistoryScope::Ssh, "git status")
            .unwrap()
            .items[0]
            .id,
        first
    );
    drop(db);
    let mut db = Database::open(directory.path()).unwrap();
    assert_eq!(
        db.load()
            .unwrap()
            .history
            .list(HistoryScope::Ssh)
            .map(|i| i.id)
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    db.delete_history(&[first]).unwrap();
    drop(db);
    let db = Database::open(directory.path()).unwrap();
    let history = db.load().unwrap().history;
    assert_eq!(history.items.len(), 2);
    assert!(history.items.iter().all(|i| i.id != first));
    assert_eq!(history.list(HistoryScope::Local).next().unwrap().id, local);
}

#[test]
fn shared_scope_does_not_include_local_or_malformed_origins() {
    assert!(HistoryScope::Ssh.includes("ssh"));
    for origin in ["local", "ssh:", "ssh:not-a-profile", "ssh-backup:1", ""] {
        assert!(
            !HistoryScope::Ssh.includes(origin),
            "unexpected origin: {origin}"
        );
    }
    assert!(HistoryScope::Local.includes("local"));
    assert!(!HistoryScope::Local.includes(&format!("ssh:{}", Id::new_v4())));
}

#[test]
fn normalization_preserves_internal_spacing_case_and_unicode() {
    let mut catalog = HistoryCatalog::default();
    assert!(catalog.record(HistoryScope::Ssh, " \n\t").is_none());
    let id = catalog
        .record(HistoryScope::Ssh, "  echo  中文 \n")
        .unwrap();
    assert_eq!(catalog.record(HistoryScope::Ssh, "echo  中文"), Some(id));
    for command in ["echo 中文", "ECHO  中文", "echo\n中文"] {
        catalog.record(HistoryScope::Ssh, command);
    }
    assert_eq!(catalog.items.len(), 4);
    catalog.validate().unwrap();
    let encoded = serde_json::to_string(&catalog).unwrap();
    assert!(!encoded.contains("timestamp"));
    assert!(!encoded.contains("scope"));
    assert_eq!(
        serde_json::from_str::<HistoryCatalog>(&encoded).unwrap(),
        catalog
    );
}

#[test]
fn capacity_is_shared_and_invalid_membership_is_rejected() {
    let mut catalog = HistoryCatalog::default();
    let first = catalog.record(HistoryScope::Local, "first").unwrap();
    for index in 0..HISTORY_LIMIT {
        catalog.record(HistoryScope::Ssh, &format!("echo {index}"));
    }
    assert_eq!(catalog.items.len(), HISTORY_LIMIT);
    assert!(!catalog.local_ids.contains(&first));
    catalog.validate().unwrap();
    catalog.local_ids.push(catalog.ssh_ids[0]);
    assert!(catalog.validate().is_err());
}

#[test]
fn visible_selection_supports_plain_shift_and_additive_clicks() {
    let visible = [Id::new_v4(), Id::new_v4(), Id::new_v4(), Id::new_v4()];
    let mut selected = HashSet::new();
    let mut anchor = None;

    update_visible_selection(
        &visible,
        &mut selected,
        &mut anchor,
        visible[1],
        false,
        false,
    );
    assert_eq!(
        selected.iter().copied().collect::<Vec<_>>(),
        vec![visible[1]]
    );
    assert_eq!(anchor, Some(visible[1]));

    update_visible_selection(
        &visible,
        &mut selected,
        &mut anchor,
        visible[3],
        true,
        false,
    );
    assert_eq!(
        visible[1..=3]
            .iter()
            .filter(|id| selected.contains(*id))
            .count(),
        3
    );

    update_visible_selection(
        &visible,
        &mut selected,
        &mut anchor,
        visible[0],
        false,
        true,
    );
    assert!(selected.contains(&visible[0]));
    update_visible_selection(
        &visible,
        &mut selected,
        &mut anchor,
        visible[0],
        false,
        true,
    );
    assert!(!selected.contains(&visible[0]));
}
#[test]
fn history_delete_targets_include_only_explicit_visible_selection() {
    let visible = [Id::new_v4(), Id::new_v4(), Id::new_v4()];
    let hidden = Id::new_v4();
    let none = HashSet::new();
    assert!(history_delete_targets(&visible, &none).is_empty());

    let mut partial = HashSet::new();
    partial.insert(visible[1]);
    assert_eq!(history_delete_targets(&visible, &partial), vec![visible[1]]);

    let mut all = HashSet::new();
    all.extend(visible);
    assert_eq!(history_delete_targets(&visible, &all), visible);

    let mut hidden_selection = HashSet::new();
    hidden_selection.insert(hidden);
    assert!(history_delete_targets(&[visible[0], visible[2]], &hidden_selection).is_empty());
    assert!(history_delete_targets(&[], &all).is_empty());

    let mut db = Database::memory().unwrap();
    let first = db
        .record_history(HistoryScope::Ssh, "first frozen command")
        .unwrap()
        .items[0]
        .id;
    let second = db
        .record_history(HistoryScope::Ssh, "second frozen command")
        .unwrap()
        .items[0]
        .id;
    let outside = db
        .record_history(HistoryScope::Ssh, "outside current search")
        .unwrap()
        .items[0]
        .id;
    let frozen = history_delete_targets(&[first, second], &HashSet::from([first, second, outside]));
    let later = db
        .record_history(HistoryScope::Ssh, "added after confirmation")
        .unwrap()
        .items[0]
        .id;
    let remaining = db.delete_history(&frozen).unwrap();
    assert_eq!(
        remaining.items.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![later, outside]
    );
}
