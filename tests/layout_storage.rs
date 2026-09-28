use mantash::{
    encoding::Encoding,
    layout::{PaneLayout, tool_width},
    model::*,
    storage::Database,
};
use rusqlite::Connection;
use serde_json::json;

fn local() -> SessionSpec {
    SessionSpec::Local {
        shell: "/bin/sh".into(),
        directory: "/tmp/资料".into(),
        encoding: Encoding::Gb18030,
    }
}

#[test]
fn mixed_local_tree_caps_and_closes_without_replacing_other_panes() {
    let first = Id::new_v4();
    let mut tree = PaneLayout::single(first);
    let mut active = first;
    for axis in [
        Split::Horizontal,
        Split::Vertical,
        Split::Horizontal,
        Split::Vertical,
    ] {
        let added = Id::new_v4();
        assert!(tree.split(active, added, axis));
        active = added;
    }
    assert_eq!(tree.panes().len(), 5);
    assert!(!tree.split(active, Id::new_v4(), Split::Horizontal));
    let original = tree.panes();
    let mut closed = tree.without(active).unwrap();
    assert_eq!(closed.panes(), original[..4]);
    assert!(closed.split(first, Id::new_v4(), Split::Vertical));
}
#[test]
fn unstamped_databases_adopt_the_format_stamp_and_others_are_rejected() {
    // An unstamped database (fresh or from the brief stamp-less build) adopts stamp 1.
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("mantash.sqlite3")).unwrap();
    conn.execute_batch("CREATE TABLE records(kind TEXT,id TEXT,data TEXT,PRIMARY KEY(kind,id));CREATE TABLE trust(host TEXT,port INTEGER,fingerprint TEXT,PRIMARY KEY(host,port));PRAGMA user_version=0;").unwrap();
    conn.execute(
        "INSERT INTO records VALUES('preferences','current','{}')",
        [],
    )
    .unwrap();
    drop(conn);
    let db = Database::open(dir.path()).unwrap();
    assert!(db.load().is_ok());
    drop(db);
    let conn = Connection::open(dir.path().join("mantash.sqlite3")).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        1
    );
    // Any other stamp is rejected without touching the file.
    let foreign = tempfile::tempdir().unwrap();
    let conn = Connection::open(foreign.path().join("mantash.sqlite3")).unwrap();
    conn.execute_batch("PRAGMA user_version=999;").unwrap();
    drop(conn);
    assert!(Database::open(foreign.path()).is_err());
    let conn = Connection::open(foreign.path().join("mantash.sqlite3")).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        999
    );
}
#[test]
fn invalid_v2_graph_falls_back_to_individual_tabs_without_losing_specs() {
    let a = SavedPane::new(local());
    let b = SavedPane::new(local());
    let active = b.id;
    let raw = json!({"tabs":[{"id":Id::new_v4(),"panes":[a,b],"active_pane":active,"layout":{"kind":"unknown"}}],"active_tab":0});
    let mut workspace: Workspace = serde_json::from_value(raw).unwrap();
    workspace.normalize();
    assert_eq!(workspace.tabs.len(), 2);
    assert_eq!(workspace.active_tab, 1);
    assert_eq!(workspace.tabs[1].active_pane, active);
}
#[test]
fn window_constraints_do_not_overwrite_saved_widths() {
    let preferred = Some(700.);
    assert_eq!(tool_width(preferred, 1510., 1468.), 700.);
    assert_eq!(tool_width(preferred, 960., 942.), 480.);
    assert_eq!(tool_width(preferred, 1510., 1468.), 700.);
    // No saved preference uses the compact minimum; an explicit preference
    // remains the value restored after reopening the sidebar.
    assert_eq!(tool_width(None, 1280., 1238.), 280.);
    assert_eq!(tool_width(None, 960., 942.), 280.);
}
