use mantash::model::Preferences;

#[test]
fn file_panel_height_is_normalized_when_persisted() {
    let mut prefs = Preferences::default();
    prefs.files_preferred_height = Some(f32::NAN);
    prefs.normalize();
    assert_eq!(prefs.files_preferred_height, None);

    prefs.files_preferred_height = Some(80.);
    prefs.normalize();
    assert_eq!(prefs.files_preferred_height, Some(120.));

    prefs.files_preferred_height = Some(900.);
    prefs.normalize();
    assert_eq!(prefs.files_preferred_height, Some(600.));
}
