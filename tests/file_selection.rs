use mantash::file_selection::RowSelection;

/// Deliberately non-alphabetical paths make accidental sorting/index reuse detectable.
fn visible() -> Vec<String> {
    ["/srv/z", "/srv/a", "/srv/中文.txt", "/srv/folder", "/srv/b"]
        .map(str::to_owned)
        .to_vec()
}

#[test]
fn plain_click_replaces_and_repeated_click_keeps_one_row_selected() {
    let rows = visible();
    let mut selection = RowSelection::default();
    selection.click(&rows, &rows[0], false, false);
    selection.click(&rows, &rows[2], false, false);
    selection.click(&rows, &rows[2], false, false);
    assert_eq!(selection.len(), 1);
    assert!(selection.contains(&rows[2]));
}

#[test]
fn shift_ranges_follow_visible_order_and_keep_the_original_pivot() {
    let rows = visible();
    let mut selection = RowSelection::default();
    selection.click(&rows, &rows[2], false, false);
    selection.click(&rows, &rows[4], true, false);
    assert_eq!(selection.len(), 3);
    assert!(rows[2..].iter().all(|path| selection.contains(path)));
    selection.click(&rows, &rows[0], true, false);
    assert_eq!(selection.len(), 3);
    assert!(rows[..=2].iter().all(|path| selection.contains(path)));
    assert!(!selection.contains(&rows[4]));
    selection.click(&rows, &rows[1], true, false);
    assert_eq!(selection.len(), 2);
    assert_eq!(selection.anchor(), Some(rows[2].as_str()));
}

#[test]
fn additive_click_toggles_and_additive_shift_keeps_unrelated_selections() {
    let rows = visible();
    let mut selection = RowSelection::default();
    selection.click(&rows, &rows[0], false, false);
    selection.click(&rows, &rows[3], false, true);
    selection.click(&rows, &rows[4], true, true);
    assert_eq!(selection.len(), 3);
    assert!(selection.contains(&rows[0]));
    assert!(selection.contains(&rows[3]));
    assert!(selection.contains(&rows[4]));
    selection.click(&rows, &rows[3], false, true);
    assert!(!selection.contains(&rows[3]));
    assert_eq!(selection.len(), 2);
}

#[test]
fn filtering_out_an_anchor_prunes_hidden_targets_and_starts_a_new_range() {
    let rows = visible();
    let mut selection = RowSelection::default();
    selection.click(&rows, &rows[2], false, false);
    selection.click(&rows, &rows[4], true, false);
    let filtered = vec![
        rows[0].clone(),
        rows[1].clone(),
        rows[3].clone(),
        rows[4].clone(),
    ];
    selection.retain_visible(&filtered);
    assert!(!selection.contains(&rows[2]));
    assert_eq!(selection.anchor(), None);
    selection.click(&filtered, &rows[1], true, false);
    assert_eq!(selection.len(), 1);
    assert!(selection.contains(&rows[1]));
}

#[test]
fn invalid_rows_cannot_change_selection_or_anchor() {
    let rows = visible();
    let mut selection = RowSelection::default();
    selection.click(&rows, &rows[1], false, false);
    let before = selection.clone();
    assert!(!selection.click(&rows, "/another-host/other", true, true));
    assert_eq!(selection, before);
}

#[test]
fn clearing_and_select_all_have_predictable_range_pivots() {
    let rows = visible();
    let mut selection = RowSelection::default();
    selection.select_all(&rows);
    assert_eq!(selection.len(), rows.len());
    assert_eq!(selection.anchor(), Some(rows[0].as_str()));
    selection.clear();
    assert!(selection.is_empty());
    assert_eq!(selection.anchor(), None);
    selection.click(&rows, &rows[3], true, false);
    assert_eq!(selection.len(), 1);
    selection.select_all(&[]);
    assert!(selection.is_empty());
    assert_eq!(selection.anchor(), None);
}

#[test]
fn independent_views_do_not_share_ranges_even_for_identical_paths() {
    let rows = visible();
    let mut first = RowSelection::default();
    let mut second = RowSelection::default();
    first.click(&rows, &rows[0], false, false);
    second.click(&rows, &rows[4], false, false);
    first.click(&rows, &rows[2], true, false);
    assert_eq!(first.len(), 3);
    assert_eq!(second.len(), 1);
    assert_eq!(second.anchor(), Some(rows[4].as_str()));
}
