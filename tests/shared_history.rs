//! Shared SSH history includes old sources after restart, without mixing local or unknown records.
use mantash::{model::*, storage::Database};
use std::collections::HashSet;

#[test]
fn legacy_ssh_origins_share_a_list_after_database_reload() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data");
    let source_a = Id::new_v4();
    let source_b = Id::new_v4();
    let entries = [
        HistoryEntry {
            id: Id::new_v4(),
            scope: format!("ssh:{source_a}"),
            command: "git status".into(),
            timestamp: 20,
        },
        HistoryEntry {
            id: Id::new_v4(),
            scope: format!("ssh:{source_b}"),
            command: "kubectl get pods".into(),
            timestamp: 30,
        },
        HistoryEntry {
            id: Id::new_v4(),
            scope: "local".into(),
            command: "local-only".into(),
            timestamp: 40,
        },
    ];
    {
        let database = Database::open(&path).unwrap();
        for entry in &entries {
            database.touch_history(entry).unwrap();
        }
    }
    let mut database = Database::open(&path).unwrap();
    let snapshot = database.load().unwrap();
    let shared = snapshot
        .history
        .iter()
        .filter(|entry| HistoryScope::Ssh.includes(&entry.scope))
        .collect::<Vec<_>>();
    assert_eq!(
        shared.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        vec![entries[1].id, entries[0].id]
    );
    // Deleting one shared record by exact UUID never removes another source's record.
    database.delete_history(&[entries[1].id]).unwrap();
    let remaining = database.load().unwrap().history;
    assert!(remaining.iter().any(|entry| entry.id == entries[0].id));
    assert!(remaining.iter().any(|entry| entry.id == entries[2].id));
    assert_eq!(remaining.len(), 2);
    // Identical commands collapse to the newest copy within the shared SSH list.
    let duplicate = HistoryEntry {
        id: Id::new_v4(),
        scope: format!("ssh:{source_b}"),
        command: "git status".into(),
        timestamp: 50,
    };
    database.touch_history(&duplicate).unwrap();
    let deduplicated = database.load().unwrap().history;
    let same_command: Vec<_> = deduplicated
        .iter()
        .filter(|entry| entry.command == "git status")
        .collect();
    assert_eq!(same_command.len(), 1);
    assert_eq!(same_command[0].id, duplicate.id);
    assert!(deduplicated.iter().any(|entry| entry.id == entries[2].id));
    assert_eq!(deduplicated.len(), 2);
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
fn unknown_history_origins_do_not_deduplicate_with_shared_ssh_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data");
    let valid = HistoryEntry {
        id: Id::new_v4(),
        scope: format!("ssh:{}", Id::new_v4()),
        command: "same command".into(),
        timestamp: 1,
    };
    let unknown = HistoryEntry {
        id: Id::new_v4(),
        scope: "ssh-invalid".into(),
        command: "same command".into(),
        timestamp: 2,
    };
    let newest_valid = HistoryEntry {
        id: Id::new_v4(),
        scope: format!("ssh:{}", Id::new_v4()),
        command: "same command".into(),
        timestamp: 3,
    };
    let mut database = Database::open(&path).unwrap();
    database.touch_history(&valid).unwrap();
    database.touch_history(&unknown).unwrap();
    database.touch_history(&newest_valid).unwrap();
    database.compact_history().unwrap();
    let entries = database.load().unwrap().history;
    assert!(entries.iter().any(|entry| entry.id == unknown.id));
    assert!(entries.iter().any(|entry| entry.id == newest_valid.id));
    assert!(!entries.iter().any(|entry| entry.id == valid.id));
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

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data");
    let scope = format!("ssh:{}", Id::new_v4());
    let first = HistoryEntry {
        id: visible[0],
        scope: scope.clone(),
        command: "first frozen command".into(),
        timestamp: 1,
    };
    let second = HistoryEntry {
        id: visible[1],
        scope: scope.clone(),
        command: "second frozen command".into(),
        timestamp: 2,
    };
    let outside_search = HistoryEntry {
        id: visible[2],
        scope: scope.clone(),
        command: "outside current search".into(),
        timestamp: 3,
    };
    let mut database = Database::open(&path).unwrap();
    for entry in [&first, &second, &outside_search] {
        database.touch_history(entry).unwrap();
    }
    let mut selected = HashSet::new();
    selected.insert(first.id);
    selected.insert(second.id);
    let frozen_targets = history_delete_targets(&[first.id, second.id], &selected);
    let added_after_confirmation = HistoryEntry {
        id: Id::new_v4(),
        scope,
        command: "added after confirmation".into(),
        timestamp: 4,
    };
    database.touch_history(&added_after_confirmation).unwrap();
    database.delete_history(&frozen_targets).unwrap();

    let remaining = database.load().unwrap().history;
    assert!(!remaining.iter().any(|entry| entry.id == first.id));
    assert!(!remaining.iter().any(|entry| entry.id == second.id));
    assert!(remaining.iter().any(|entry| entry.id == outside_search.id));
    assert!(
        remaining
            .iter()
            .any(|entry| entry.id == added_after_confirmation.id)
    );
}
