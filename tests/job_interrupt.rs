//! Decision rules for following up a keyboard interrupt of a local foreground job.
use mantash::job_interrupt::{
    FOLLOW_UP_WINDOW, FollowUp, InterruptFollowUp, JobProbe, POLL_INTERVAL,
};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

const SHELL: i32 = 100;
const JOB: i32 = 200;

struct Fake {
    foreground: Cell<Option<i32>>,
    interrupt: Cell<Option<u8>>,
    leader_alive: Cell<bool>,
    group_alive: Cell<bool>,
    hung_up: RefCell<Vec<i32>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            foreground: Cell::new(Some(JOB)),
            interrupt: Cell::new(Some(3)),
            leader_alive: Cell::new(true),
            group_alive: Cell::new(true),
            hung_up: RefCell::new(vec![]),
        }
    }
    /// The job leader exited, the shell reclaimed the terminal and members remain.
    fn leave_leftovers(&self) {
        self.leader_alive.set(false);
        self.foreground.set(Some(SHELL));
    }
}

impl JobProbe for Fake {
    fn foreground_group(&self) -> Option<i32> {
        self.foreground.get()
    }
    fn interrupt_char(&self) -> Option<u8> {
        self.interrupt.get()
    }
    fn process_exists(&self, pid: i32) -> bool {
        pid == JOB && self.leader_alive.get()
    }
    fn group_exists(&self, pgid: i32) -> bool {
        pgid == JOB && self.group_alive.get()
    }
    fn hang_up_group(&self, pgid: i32) -> bool {
        self.hung_up.borrow_mut().push(pgid);
        true
    }
}

fn interrupted(probe: &Fake, now: Instant) -> InterruptFollowUp {
    let mut follow_up = InterruptFollowUp::new(SHELL);
    follow_up.input(b"\x03", probe, now);
    assert!(follow_up.is_pending());
    follow_up
}

#[test]
fn leftovers_of_an_ended_interrupted_job_are_hung_up_once() {
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    assert_eq!(follow_up.poll(&probe, start), FollowUp::Waiting);
    probe.leave_leftovers();
    assert_eq!(
        follow_up.poll(&probe, start + POLL_INTERVAL),
        FollowUp::HungUp(JOB)
    );
    assert_eq!(
        follow_up.poll(&probe, start + POLL_INTERVAL * 3),
        FollowUp::Idle
    );
    assert_eq!(*probe.hung_up.borrow(), vec![JOB]);
}

#[test]
fn polling_is_throttled() {
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    assert_eq!(follow_up.poll(&probe, start), FollowUp::Waiting);
    probe.leave_leftovers();
    assert_eq!(
        follow_up.poll(&probe, start + POLL_INTERVAL / 2),
        FollowUp::Waiting
    );
    assert!(probe.hung_up.borrow().is_empty());
    assert_eq!(
        follow_up.poll(&probe, start + POLL_INTERVAL),
        FollowUp::HungUp(JOB)
    );
}

#[test]
fn input_without_the_interrupt_character_is_ignored() {
    let probe = Fake::new();
    let mut follow_up = InterruptFollowUp::new(SHELL);
    follow_up.input(b"ls\r", &probe, Instant::now());
    assert!(!follow_up.is_pending());
}

#[test]
fn raw_mode_programs_receive_the_byte_without_follow_up() {
    // vim, ssh and other raw-mode programs consume ^C themselves.
    let probe = Fake::new();
    probe.interrupt.set(None);
    let mut follow_up = InterruptFollowUp::new(SHELL);
    follow_up.input(b"\x03", &probe, Instant::now());
    assert!(!follow_up.is_pending());
}

#[test]
fn a_custom_interrupt_character_is_honoured() {
    let probe = Fake::new();
    probe.interrupt.set(Some(0x07));
    let mut follow_up = InterruptFollowUp::new(SHELL);
    follow_up.input(b"\x03", &probe, Instant::now());
    assert!(!follow_up.is_pending());
    follow_up.input(b"\x07", &probe, Instant::now());
    assert!(follow_up.is_pending());
}

#[test]
fn interrupt_at_the_prompt_never_targets_the_shell() {
    let probe = Fake::new();
    probe.foreground.set(Some(SHELL));
    let mut follow_up = InterruptFollowUp::new(SHELL);
    follow_up.input(b"\x03", &probe, Instant::now());
    assert!(!follow_up.is_pending());
    probe.foreground.set(None);
    follow_up.input(b"\x03", &probe, Instant::now());
    assert!(!follow_up.is_pending());
}

#[test]
fn a_stopped_or_backgrounded_job_is_left_alone() {
    // Ctrl+Z: the shell reclaims the terminal but the job leader still exists.
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    probe.foreground.set(Some(SHELL));
    assert_eq!(follow_up.poll(&probe, start), FollowUp::Released);
    assert!(probe.hung_up.borrow().is_empty());
}

#[test]
fn a_fully_exited_job_needs_no_signal() {
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    probe.leave_leftovers();
    probe.group_alive.set(false);
    assert_eq!(follow_up.poll(&probe, start), FollowUp::Released);
    assert!(probe.hung_up.borrow().is_empty());
}

#[test]
fn a_program_that_survives_the_interrupt_is_released_after_the_window() {
    // A REPL or pager handles ^C and keeps the terminal; its later exit is not an interrupt.
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    assert_eq!(
        follow_up.poll(&probe, start + FOLLOW_UP_WINDOW),
        FollowUp::Waiting
    );
    let late = start + FOLLOW_UP_WINDOW + POLL_INTERVAL;
    assert_eq!(follow_up.poll(&probe, late), FollowUp::Released);
    probe.leave_leftovers();
    assert_eq!(follow_up.poll(&probe, late + POLL_INTERVAL), FollowUp::Idle);
    assert!(probe.hung_up.borrow().is_empty());
}

#[test]
fn a_repeated_interrupt_restarts_the_window() {
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    let again = start + FOLLOW_UP_WINDOW;
    follow_up.input(b"\x03", &probe, again);
    probe.leave_leftovers();
    assert_eq!(
        follow_up.poll(&probe, again + POLL_INTERVAL),
        FollowUp::HungUp(JOB)
    );
}

#[test]
fn an_unreadable_terminal_drops_the_pending_interrupt() {
    let probe = Fake::new();
    let start = Instant::now();
    let mut follow_up = interrupted(&probe, start);
    probe.foreground.set(None);
    probe.leader_alive.set(false);
    assert_eq!(follow_up.poll(&probe, start), FollowUp::Released);
    assert!(probe.hung_up.borrow().is_empty());
}
