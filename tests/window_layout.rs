use mantash::window_layout::{Command, Position, Rect, target};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn presets_stay_inside_work_area_with_menu_and_dock_offsets() {
    let area = rect(0., 38., 1512., 878.);
    let current = rect(50., 80., 1200., 700.);
    for command in [
        Command::Compact,
        Command::Standard,
        Command::Wide,
        Command::Fill,
    ] {
        let result = target(command, current, area, None).unwrap();
        assert!(result.x >= area.x && result.y >= area.y);
        assert!(result.x + result.width <= area.x + area.width + 0.5);
        assert!(result.y + result.height <= area.y + area.height + 0.5);
        assert_eq!(result.width, result.width.round());
    }
}

#[test]
fn positioning_keeps_size_and_supports_negative_display_coordinates() {
    let area = rect(-1920., -300., 1920., 1080.);
    let current = rect(-1700., -200., 960., 640.);
    assert_eq!(
        target(
            Command::Position {
                position: Position::BottomRight
            },
            current,
            area,
            None
        )
        .unwrap(),
        rect(-960., 140., 960., 640.)
    );
    assert_eq!(
        target(
            Command::Position {
                position: Position::Center
            },
            current,
            area,
            None
        )
        .unwrap(),
        rect(-1440., -80., 960., 640.)
    );
}

#[test]
fn restore_preserves_original_geometry_but_rehomes_offscreen_rectangles() {
    let area = rect(0., 30., 1280., 750.);
    let saved = rect(100., 50., 960., 640.);
    assert_eq!(
        target(Command::Restore, area, area, Some(saved)).unwrap(),
        saved
    );
    let moved = target(
        Command::Restore,
        area,
        area,
        Some(rect(-5000., 5000., 1440., 900.)),
    )
    .unwrap();
    assert_eq!(moved, area);
}

#[test]
fn invalid_inputs_and_missing_restore_are_rejected() {
    let area = rect(0., 0., 1920., 1080.);
    for (width, height) in [
        (f32::NAN, 800.),
        (1280., f32::INFINITY),
        (0., 800.),
        (1280., 100.),
        (20000., 800.),
    ] {
        assert!(target(Command::Custom { width, height }, area, area, None).is_err());
    }
    assert!(target(Command::Restore, area, area, None).is_err());
}
