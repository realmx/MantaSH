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

#[test]
fn new_defaults_and_reset_preserve_business_and_window_preferences() {
    use mantash::model::Theme;
    let mut prefs: Preferences = serde_json::from_str("{}").unwrap();
    assert!(!prefs.follow_system_theme);
    assert!(prefs.mouse_reporting && prefs.open_files_on_connect && prefs.remember_credentials);
    prefs.follow_system_theme = true;
    prefs.system_theme = Some(Theme::Night);
    let json = serde_json::to_value(&prefs).unwrap();
    assert!(json.get("system_theme").is_none());
    assert_eq!(prefs.effective_theme(), Theme::Night);
    prefs.files_preferred_height = Some(350.);
    prefs.export_directory = Some("/fixture/export".into());
    prefs.mouse_reporting = false;
    prefs.remember_credentials = false;
    prefs.restore_visible_defaults();
    assert!(prefs.mouse_reporting && prefs.remember_credentials);
    assert_eq!(prefs.files_preferred_height, Some(350.));
    assert_eq!(prefs.export_directory.as_deref(), Some("/fixture/export"));
}

#[test]
fn retired_ssh_port_preference_is_ignored_without_resetting_other_settings() {
    let mut prefs: Preferences = serde_json::from_value(serde_json::json!({
        "default_ssh_port": 2222,
        "follow_system_theme": true,
        "mouse_reporting": false,
        "open_files_on_connect": false,
        "remember_credentials": false,
        "ui_size": 16.
    }))
    .unwrap();
    prefs.normalize();
    assert!(prefs.follow_system_theme);
    assert!(!prefs.mouse_reporting && !prefs.open_files_on_connect && !prefs.remember_credentials);
    assert_eq!(prefs.ui_size, 16.);
    assert!(
        serde_json::to_value(&prefs)
            .unwrap()
            .get("default_ssh_port")
            .is_none()
    );
}
