//! Output wakeup coalescing and precise input deltas, independent of the native view.
use std::sync::atomic::{AtomicBool, Ordering};

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
