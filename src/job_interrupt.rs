//! Follow-through for a local foreground job interrupted from the keyboard.
//!
//! The PTY line discipline already delivers SIGINT to the whole foreground
//! process group. A job can still leave members behind in that group: `&`
//! children of a non-interactive `sh -c` ignore SIGINT, and a child whose
//! SIGINT handler stalls outlives a wrapper such as pnpm that gives up. The
//! shell then reclaims the terminal while those members keep ports or files.
//!
//! After the user's interrupt, once the interrupted group leaves the
//! foreground and its leader no longer exists, the leftover members receive one
//! SIGHUP, the signal a terminal hangup delivers. `nohup` keeps its meaning,
//! nothing outside the interrupted group is signalled, and processes that moved
//! to their own group or session (`detached`, `setsid`) are left alone.

use std::time::{Duration, Instant};

/// The interrupted group must leave the foreground within this window. A
/// program that handles the interrupt and keeps running is then left alone.
pub const FOLLOW_UP_WINDOW: Duration = Duration::from_secs(10);
/// Minimum spacing between foreground-group checks while an interrupt is pending.
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Operating-system queries and the single signal used by [`InterruptFollowUp`].
pub trait JobProbe {
    /// Foreground process group of the terminal, if it can be read.
    fn foreground_group(&self) -> Option<i32>;
    /// The INTR control character while the line discipline generates signals.
    /// `None` in raw mode or when ISIG is disabled: no SIGINT is generated then.
    fn interrupt_char(&self) -> Option<u8>;
    /// Whether a process with this PID exists, including one we cannot signal.
    fn process_exists(&self, pid: i32) -> bool;
    /// Whether this process group still has members we may signal.
    fn group_exists(&self, pgid: i32) -> bool;
    /// Send SIGHUP to the process group once. Returns whether it was delivered.
    fn hang_up_group(&self, pgid: i32) -> bool;
}

/// Result of one [`InterruptFollowUp::poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowUp {
    /// No interrupt is pending.
    Idle,
    /// The interrupted group is still in the foreground, or the check is throttled.
    Waiting,
    /// The pending interrupt was dropped without sending a signal.
    Released,
    /// Leftover members of this group were sent SIGHUP.
    HungUp(i32),
}

/// Tracks at most one interrupted foreground group for a local shell session.
#[derive(Debug)]
pub struct InterruptFollowUp {
    shell_group: i32,
    pending: Option<(i32, Instant)>,
    last_poll: Option<Instant>,
}

impl InterruptFollowUp {
    /// `shell_group` is the shell's process group; it is never signalled.
    pub fn new(shell_group: i32) -> Self {
        Self {
            shell_group,
            pending: None,
            last_poll: None,
        }
    }

    /// Whether an interrupted group is still being tracked.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Inspect user input before it is written to the PTY. The foreground group
    /// must be read first: the signal can end the job before the next poll.
    pub fn input(&mut self, data: &[u8], probe: &impl JobProbe, now: Instant) {
        let Some(interrupt) = probe.interrupt_char() else {
            return;
        };
        if !data.contains(&interrupt) {
            return;
        }
        match probe.foreground_group() {
            Some(group) if group > 1 && group != self.shell_group => {
                self.pending = Some((group, now));
                self.last_poll = None;
            }
            _ => {}
        }
    }

    /// Advance the pending interrupt. Signals only when the interrupted group
    /// left the foreground, its leader is gone and members remain.
    pub fn poll(&mut self, probe: &impl JobProbe, now: Instant) -> FollowUp {
        let Some((group, since)) = self.pending else {
            return FollowUp::Idle;
        };
        if self
            .last_poll
            .is_some_and(|last| now.saturating_duration_since(last) < POLL_INTERVAL)
        {
            return FollowUp::Waiting;
        }
        self.last_poll = Some(now);
        let expired = now.saturating_duration_since(since) > FOLLOW_UP_WINDOW;
        match probe.foreground_group() {
            Some(foreground) if foreground == group => {
                if expired {
                    self.pending = None;
                    FollowUp::Released
                } else {
                    FollowUp::Waiting
                }
            }
            None => {
                self.pending = None;
                FollowUp::Released
            }
            Some(_) => {
                self.pending = None;
                // A living leader means the job was stopped or moved to the
                // background, not ended; a reused PID errs the same safe way.
                if expired || probe.process_exists(group) || !probe.group_exists(group) {
                    return FollowUp::Released;
                }
                if probe.hang_up_group(group) {
                    FollowUp::HungUp(group)
                } else {
                    FollowUp::Released
                }
            }
        }
    }
}

/// [`JobProbe`] backed by a local PTY master descriptor.
#[cfg(unix)]
pub struct PtyProbe {
    fd: std::os::fd::RawFd,
}

#[cfg(unix)]
impl PtyProbe {
    /// `fd` must stay open for the probe's lifetime.
    pub fn new(fd: std::os::fd::RawFd) -> Self {
        Self { fd }
    }
}

#[cfg(unix)]
impl JobProbe for PtyProbe {
    fn foreground_group(&self) -> Option<i32> {
        let group = unsafe { libc::tcgetpgrp(self.fd) };
        (group > 0).then_some(group)
    }

    fn interrupt_char(&self) -> Option<u8> {
        let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(self.fd, termios.as_mut_ptr()) } != 0 {
            return None;
        }
        let termios = unsafe { termios.assume_init() };
        let interrupt = termios.c_cc[libc::VINTR];
        // _POSIX_VDISABLE is 0 on Linux and 0xff on macOS.
        ((termios.c_lflag & libc::ISIG) != 0 && interrupt != 0 && interrupt != 0xff)
            .then_some(interrupt)
    }

    fn process_exists(&self, pid: i32) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    fn group_exists(&self, pgid: i32) -> bool {
        pgid > 1 && unsafe { libc::kill(-pgid, 0) == 0 }
    }

    fn hang_up_group(&self, pgid: i32) -> bool {
        pgid > 1 && unsafe { libc::killpg(pgid, libc::SIGHUP) == 0 }
    }
}
