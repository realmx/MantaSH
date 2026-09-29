//! Output wakeup coalescing and precise input deltas, independent of the native view.
use std::sync::atomic::{AtomicBool, Ordering};

/// Commit local grid geometry only after the OS PTY accepts the same size.
/// Both local workers use this under their shared resize/output lock. Do not
/// hold the terminal lock during the OS call: native painting must stay free.
pub(crate) fn resize_local_pty<E>(
    terminal: &parking_lot::Mutex<crate::terminal::TerminalBuffer>,
    size: crate::terminal::GridSize,
    resize: impl FnOnce(portable_pty::PtySize) -> Result<(), E>,
) -> Result<bool, E> {
    resize(portable_pty::PtySize {
        cols: size.cols as u16,
        rows: size.rows as u16,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    Ok(terminal.lock().resize_local(size.cols, size.rows))
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    use crate::{
        encoding::Encoding,
        terminal::{GridSize, TerminalBuffer},
    };
    use alacritty_terminal::grid::Dimensions;
    use parking_lot::Mutex;

    #[test]
    fn failed_pty_resize_preserves_grid_and_success_commits_local_semantics() {
        let terminal = Mutex::new(TerminalBuffer::new_local(Encoding::Utf8));
        {
            let mut buffer = terminal.lock();
            buffer.resize_local(282, 2);
            buffer.set_shell_token("resize-fixture".into());
            buffer.feed(b"$ \x1b]777;mantash-cursor;resize-fixture;ready\x07");
            buffer.feed(b"\x1b[260G[right prompt]\x1b[3G");
        }
        let size = GridSize { cols: 174, rows: 1 };
        let revision = terminal.lock().revision;
        let failed = resize_local_pty(&terminal, size, |requested| {
            assert_eq!((requested.cols, requested.rows), (174, 1));
            let buffer = terminal
                .try_lock()
                .expect("OS resize must not lock the grid");
            assert_eq!(buffer.size, GridSize { cols: 282, rows: 2 });
            Err("PTY rejected resize")
        });
        assert_eq!(failed, Err("PTY rejected resize"));
        {
            let buffer = terminal.lock();
            assert_eq!(buffer.size, GridSize { cols: 282, rows: 2 });
            assert_eq!(buffer.revision, revision);
            assert!(buffer.command_cursor.editing());
            assert_eq!(buffer.term.grid().history_size(), 0);
        }
        assert_eq!(
            resize_local_pty(&terminal, size, |_| Ok::<_, ()>(())),
            Ok(true)
        );
        let buffer = terminal.lock();
        assert_eq!(buffer.size, size);
        assert_eq!(buffer.term.grid().history_size(), 0);
    }
}

/// Forward only mouse modes requested by the foreground terminal application.
pub fn mouse_bytes(
    mode: alacritty_terminal::term::TermMode,
    button: u8,
    col: usize,
    row: usize,
    release: bool,
) -> Option<Vec<u8>> {
    use alacritty_terminal::term::TermMode;
    if !mode.intersects(TermMode::MOUSE_MODE) || col >= 65535 || row >= 65535 || button > 127 {
        return None;
    }
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(
            format!(
                "\x1b[<{button};{};{}{}",
                col + 1,
                row + 1,
                if release { 'm' } else { 'M' }
            )
            .into_bytes(),
        );
    }
    if mode.contains(TermMode::UTF8_MOUSE) && col < 2015 && row < 2015 {
        let mut bytes = vec![27, b'[', b'M', if release { 35 } else { button + 32 }];
        for value in [col + 33, row + 33] {
            let mut buffer = [0; 4];
            bytes.extend_from_slice(
                char::from_u32(value as u32)?
                    .encode_utf8(&mut buffer)
                    .as_bytes(),
            );
        }
        return Some(bytes);
    }
    (col < 223 && row < 223).then(|| {
        vec![
            27,
            b'[',
            b'M',
            if release { 35 } else { button + 32 },
            col as u8 + 33,
            row as u8 + 33,
        ]
    })
}

/// One pending frame wakeup per session bounds notification work during output floods.
#[derive(Default)]
pub struct OutputWakeup {
    pending: AtomicBool,
}
impl OutputWakeup {
    /// Returns true only for the first output until the renderer consumes a snapshot.
    pub fn mark(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }
    pub fn pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
    /// Call under the terminal lock after taking the frame, so subsequent bytes rearm it.
    pub fn acknowledge(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

/// Preserve sub-line trackpad motion; rounding each individual event loses slow scrolls.
#[derive(Default)]
pub struct ScrollAccumulator {
    remainder: f64,
    mode: u8,
}
impl ScrollAccumulator {
    /// `mode` separates scrollback, application mouse reporting and alternate-screen arrows.
    pub fn lines(&mut self, delta: f64, mode: u8) -> i32 {
        if !delta.is_finite() || delta == 0. {
            return 0;
        }
        if self.mode != mode || (self.remainder != 0. && delta.signum() != self.remainder.signum())
        {
            self.remainder = 0.;
        }
        self.mode = mode;
        self.remainder = (self.remainder + delta).clamp(-1000., 1000.);
        let lines = (self.remainder + self.remainder.signum() * 1e-12).trunc() as i32;
        self.remainder -= lines as f64;
        lines
    }
}
