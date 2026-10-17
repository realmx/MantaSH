use mantash::{editor_format, files};

#[test]
fn cleanup_is_explicit_idempotent_and_preserves_blank_lines_and_unicode() {
    for (input, expected) in [
        ("", "\n"),
        ("a  \r\n中文\t\r😀 \n\n", "a\n中文\n😀\n\n"),
        ("  indent\t", "  indent\n"),
        ("a\n\n\n", "a\n\n\n"),
    ] {
        let result = editor_format::format(input, input.len());
        assert_eq!(result.text, expected);
        assert_eq!(editor_format::format(&result.text, result.cursor), result);
        assert!(result.text.is_char_boundary(result.cursor));
    }
}
#[test]
fn cursor_clamps_trimmed_suffix_and_tracks_newline_and_unicode() {
    assert_eq!(editor_format::format("a  \r\n中", 3).cursor, 1);
    assert_eq!(editor_format::format("a  \r\n中", 5).cursor, 2);
    assert_eq!(editor_format::format("a  \r\n中", 8).cursor, 5);
}
#[test]
fn permission_modes_and_path_component_boundaries() {
    for (mode, expected) in [
        (None, "—"),
        (Some(0o100644), "-rw-r--r--"),
        (Some(0o041775), "drwxrwxr-t"),
        (Some(0o104755), "-rwsr-xr-x"),
        (Some(0o102644), "-rw-r-Sr--"),
        (Some(0o120777), "lrwxrwxrwx"),
    ] {
        assert_eq!(files::permission_text(mode), expected);
    }
    assert!(files::contains_path("/a", "/a/b"));
    assert!(files::contains_path("/a", "/a"));
    assert!(!files::contains_path("/a", "/abc"));
    assert!(!files::contains_path("/a/b", "/a"));
}

#[test]
fn selection_endpoints_map_across_crlf_and_non_bmp_text() {
    let original = "中😀  \r\n尾\t\r\n\n";
    let result = editor_format::format_selection(original, 3..17);
    assert_eq!(result.text, "中😀\n尾\n\n");
    assert_eq!(result.selection, 3..12);
    let clamped = editor_format::format_selection(original, 8..13);
    assert_eq!(clamped.selection, 7..8);
}
