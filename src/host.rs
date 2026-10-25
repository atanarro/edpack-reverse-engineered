//! Running an engine as a coroutine on its own thread.
//!
//! The original interpreters are blocking DOS programs: a main loop that waits
//! for retraces, a timer interrupt, and routines (fades, menus, animations)
//! with their own loops.  An engine keeps that structure and runs on its own
//! thread: whenever it needs time to pass ([`wait`]) it hands the current
//! frame to the host and blocks until the host advances the clock and
//! delivers input.  Host and engine never run at the same time, so play is
//! deterministic for a given input sequence.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, Once};
use std::thread::JoinHandle;

use crate::audio::Mixer;

/// Panic payload for a fatal error of the original (shown as its message).
pub struct Fatal(pub String);
/// Panic payload used to unwind the engine thread when the host goes away.
pub struct QuitSignal;
/// Panic payload for a normal end of the game from deep inside the engine
/// (EXIT, quitting from a menu).
pub struct Exit;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Escape,
}

#[derive(Clone, Copy, Debug)]
pub enum Input {
    /// Pointer position in 320x200 screen coordinates.
    MouseMove(i32, i32),
    /// Button 0 = left, 1 = right; pressed or released.
    Button(usize, bool),
    /// PC scan code (set 1) and, for text entry, the key it produced.
    KeyDown(u8, Option<Key>),
    KeyUp(u8),
}

/// What the host gets back after each advance.
pub struct Frame {
    /// 320x200 palette indices (cursor included)
    pub screen: Vec<u8>,
    /// 8-bit RGB
    pub palette: Vec<u8>,
    pub exited: bool,
    pub error: Option<String>,
}

type Job<E> = Box<dyn FnOnce(&mut E) + Send>;

enum ToEngine<E> {
    Advance { now: f64, events: Vec<Input> },
    With(Job<E>),
}

struct Link<E> {
    rx: Receiver<ToEngine<E>>,
    tx: Sender<Frame>,
}

/// The host-side state an engine embeds: the channel and the emulated clock.
pub struct Host<E> {
    link: Option<Link<E>>,
    /// emulated milliseconds made available by the host so far
    pub clock: f64,
    next_tick: f64,
    next_resume: f64,
    last: Option<f64>,
}

impl<E> Default for Host<E> {
    fn default() -> Self {
        Host { link: None, clock: 0.0, next_tick: 0.0, next_resume: 0.0, last: None }
    }
}

/// An interpreter that can run under [`spawn`].
pub trait Core: Send + Sized + 'static {
    fn host(&mut self) -> &mut Host<Self>;
    /// Period of the timer interrupt.
    fn tick_ms(&self) -> f64;
    /// The timer interrupt.
    fn isr_tick(&mut self);
    fn input_event(&mut self, ev: Input);
    /// The current screen (cursor included) and palette.
    fn frame(&mut self, error: Option<String>) -> Frame;
    /// The program itself; returns when the game ends.
    fn run(&mut self);
    fn set_exited(&mut self);
}

/// Start the engine on its own thread.
pub fn spawn<E: Core>(engine: E, mixer: Arc<Mutex<Mixer>>) -> EngineHandle<E> {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let pl = info.payload();
            if pl.downcast_ref::<Fatal>().is_none()
                && pl.downcast_ref::<QuitSignal>().is_none()
                && pl.downcast_ref::<Exit>().is_none()
            {
                default(info);
            }
        }));
    });
    let (to_engine, rx) = channel();
    let (tx, frames) = channel();
    let thread = std::thread::Builder::new()
        .name("edpack-engine".into())
        .stack_size(16 << 20)
        .spawn(move || {
            let mut e = engine;
            e.host().link = Some(Link { rx, tx });
            thread_main(&mut e);
        })
        .expect("no se pudo crear el hilo del motor");
    EngineHandle { tx: to_engine, rx: frames, thread: Some(thread), mixer, finished: false, error: None }
}

fn thread_main<E: Core>(e: &mut E) {
    let r = panic::catch_unwind(AssertUnwindSafe(|| {
        block(e);
        wait(e, 0.0);
        e.run();
    }));
    let error = match r {
        Ok(()) => None,
        Err(pl) => {
            if pl.downcast_ref::<QuitSignal>().is_some() {
                return;
            }
            if pl.downcast_ref::<Exit>().is_some() {
                None
            } else {
                Some(if let Some(Fatal(m)) = pl.downcast_ref::<Fatal>() {
                    m.clone()
                } else if let Some(s) = pl.downcast_ref::<String>() {
                    format!("Error interno: {s}")
                } else if let Some(s) = pl.downcast_ref::<&str>() {
                    format!("Error interno: {s}")
                } else {
                    "Error interno".to_string()
                })
            }
        }
    };
    e.set_exited();
    let Some(link) = e.host().link.take() else { return };
    let _ = link.tx.send(e.frame(error));
    // Keep serving inspection jobs (tests, tools) until the host hangs up.
    while let Ok(msg) = link.rx.recv() {
        match msg {
            ToEngine::With(job) => job(e),
            ToEngine::Advance { .. } => {
                let _ = link.tx.send(e.frame(None));
            }
        }
    }
}

/// Hand the current frame to the host and wait for more time / input.
/// Inspection jobs sent meanwhile run here, on the paused engine.
pub fn block<E: Core>(e: &mut E) {
    let frame = e.frame(None);
    let sent = e.host().link.as_ref().expect("the engine must run on its thread (spawn)").tx.send(frame);
    if sent.is_err() {
        panic::panic_any(QuitSignal);
    }
    loop {
        let msg = e.host().link.as_ref().unwrap().rx.recv();
        match msg {
            Ok(ToEngine::Advance { now, events }) => {
                let h = e.host();
                let last = h.last.unwrap_or(now);
                h.clock += (now - last).clamp(0.0, 250.0);
                h.last = Some(now);
                for ev in events {
                    e.input_event(ev);
                }
                return;
            }
            Ok(ToEngine::With(job)) => job(e),
            Err(_) => panic::panic_any(QuitSignal),
        }
    }
}

/// Let `ms` milliseconds of emulated time pass, running the timer interrupt
/// meanwhile (the original's waits and retrace syncs).
pub fn wait<E: Core>(e: &mut E, ms: f64) {
    e.host().next_resume += ms;
    loop {
        let h = e.host();
        if h.next_tick <= h.next_resume {
            if h.next_tick > h.clock {
                block(e);
                continue;
            }
            e.isr_tick();
            let t = e.tick_ms();
            e.host().next_tick += t;
        } else {
            if h.next_resume > h.clock {
                block(e);
                continue;
            }
            return;
        }
    }
}

/// Host-side handle to an engine running on its own thread.
pub struct EngineHandle<E> {
    tx: Sender<ToEngine<E>>,
    rx: Receiver<Frame>,
    thread: Option<JoinHandle<()>>,
    /// shared with the audio output
    pub mixer: Arc<Mutex<Mixer>>,
    /// set once the game has ended (EXIT, quit from the menu, or an error)
    pub finished: bool,
    pub error: Option<String>,
}

impl<E: Core> EngineHandle<E> {
    /// Advance emulated time to `now` (ms, any monotonic origin), delivering
    /// `events` first; returns the resulting frame.
    pub fn advance(&mut self, now: f64, events: Vec<Input>) -> Option<Frame> {
        self.tx.send(ToEngine::Advance { now, events }).ok()?;
        let f = self.rx.recv().ok()?;
        if f.exited {
            self.finished = true;
            if f.error.is_some() {
                self.error = f.error.clone();
            }
        }
        Some(f)
    }

    /// Run `job` on the engine while it is paused (inspection, tests, saving).
    pub fn with<R: Send + 'static>(&mut self, job: impl FnOnce(&mut E) -> R + Send + 'static) -> Option<R> {
        let (tx, rx) = channel();
        self.tx
            .send(ToEngine::With(Box::new(move |e| {
                let _ = tx.send(job(e));
            })))
            .ok()?;
        rx.recv().ok()
    }
}

impl<E> Drop for EngineHandle<E> {
    fn drop(&mut self) {
        // Closing the channel makes the engine thread unwind and exit.
        let (tx, _) = channel();
        drop(std::mem::replace(&mut self.tx, tx));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
