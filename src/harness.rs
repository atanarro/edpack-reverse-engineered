//! Scriptable headless driver (tests and `edtool run`).
//!
//! Script commands, separated by `;` or newlines (`#` starts a comment):
//! ```text
//! wait <ms> | waitidle | shot <name> | state | move <x> <y> | click <x> <y> | rclick
//! key <scancode> | type <text> | enter | verb <n> | do <verb> <x> <y> | use <obj> <x> <y> | combine <a> <b>
//! level <n> [door] | save <slot> <name> | load <slot>
//! expect inv|noinv <obj> | expect level <n> | expect var <n> <value> | expect exited
//! ```

use crate::Error;
use crate::engine::{Engine, EngineHandle, Frame, Input, MemStorage, Options};
use crate::game::Game;
use crate::memory::ds;

/// Callback receiving screenshots requested by `shot <name>`.
pub type ShotFn = Box<dyn FnMut(&str, &Frame)>;

pub struct Harness {
    pub handle: EngineHandle,
    pub now: f64,
    pending: Vec<Input>,
    pub last: Option<Frame>,
    pub on_shot: Option<ShotFn>,
    pub log: Box<dyn FnMut(&str)>,
}

impl Harness {
    pub fn new(game: Game) -> Result<Harness, Error> {
        let engine = Engine::new(game, Box::new(MemStorage::default()), Options::default())?;
        Ok(Harness {
            handle: engine.spawn(),
            now: 0.0,
            pending: Vec::new(),
            last: None,
            on_shot: None,
            log: Box::new(|_| {}),
        })
    }

    pub fn exited(&self) -> bool {
        self.handle.finished
    }

    fn check_error(&self) -> Result<(), Error> {
        match &self.handle.error {
            Some(e) => Err(Error::new(format!("el juego terminó con un error: {e}"))),
            None => Ok(()),
        }
    }

    pub fn step(&mut self, ms: f64) -> Result<(), Error> {
        let end = self.now + ms;
        while self.now < end && !self.handle.finished {
            self.now += 16.0;
            let ev = std::mem::take(&mut self.pending);
            self.last = self.handle.advance(self.now, ev);
        }
        self.check_error()
    }

    fn input(&mut self, ev: Input) {
        self.pending.push(ev);
    }

    /// Read engine memory (u8) at `a`.
    pub fn u8(&mut self, a: usize) -> i32 {
        self.handle.with(move |e| e.mem.u8(a)).unwrap_or(0)
    }
    pub fn i16(&mut self, a: usize) -> i32 {
        self.handle.with(move |e| e.mem.i16(a)).unwrap_or(0)
    }
    pub fn var(&mut self, n: i32) -> i32 {
        self.handle.with(move |e| e.mem.get_var(n)).unwrap_or(0)
    }
    fn verb(&mut self) -> i32 {
        self.u8(0xfd2c)
    }

    fn rclick(&mut self) -> Result<(), Error> {
        self.input(Input::Button(1, true));
        self.step(60.0)?;
        self.input(Input::Button(1, false));
        self.step(40.0)
    }
    fn click(&mut self, x: i32, y: i32) -> Result<(), Error> {
        self.input(Input::MouseMove(x, y));
        self.step(60.0)?;
        self.input(Input::Button(0, true));
        self.step(60.0)?;
        self.input(Input::Button(0, false));
        self.step(40.0)
    }
    fn mv(&mut self, x: i32, y: i32, ms: f64) -> Result<(), Error> {
        self.input(Input::MouseMove(x, y));
        self.step(ms)
    }
    fn set_verb(&mut self, n: i32) -> Result<(), Error> {
        for _ in 0..6 {
            if self.verb() == n {
                break;
            }
            self.rclick()?;
        }
        Ok(())
    }

    pub fn slot_of(&mut self, obj: i32) -> Option<usize> {
        self.handle
            .with(move |e| (0..30).find(|&s| e.mem.u8(0x8a95 + s * 6) != 0 && e.mem.i16(0x8a97 + s * 6) == obj))
            .flatten()
    }

    fn busy(&mut self) -> bool {
        self.handle
            .with(|e| {
                let m = &e.mem;
                m.u8(ds::CUR_ACTION) != 0
                    || m.i16(0xfca9) != 0
                    || m.u8(0xfcce) != 0
                    || m.i16(0xfcf0) != -1
                    || m.u8(0xfc9d) != m.u8(ds::LEVEL)
                    || m.i16(0xfd46) != 0
            })
            .unwrap_or(false)
    }

    fn wait_idle(&mut self) -> Result<(), Error> {
        let end = self.now + 60000.0;
        self.step(100.0)?;
        while !self.handle.finished && self.now < end && self.busy() {
            self.step(50.0)?;
        }
        self.step(200.0)
    }

    fn pick_up(&mut self, obj: i32) -> Result<(), Error> {
        self.mv(160, 0, 300.0)?;
        if self.u8(0xfd2b) == 0 {
            return Err(Error::new("el inventario no se abrió"));
        }
        self.set_verb(3)?;
        let s = self.slot_of(obj).ok_or_else(|| Error::new(format!("el objeto {obj} no está en el inventario")))?;
        while s >= self.u8(0xfccc) as usize + 8 {
            self.click(315, 15)?;
        }
        let x = (s as i32 - self.u8(0xfccc)) * 37 + 30;
        self.mv(x, 15, 200.0)?;
        self.click(x, 15)?;
        self.step(100.0)?;
        if self.u8(0x8a8c) != 1 {
            return Err(Error::new(format!("no se pudo coger el objeto {obj}")));
        }
        Ok(())
    }

    pub fn state(&mut self) -> String {
        let now = self.now;
        self.handle
            .with(move |e| {
                let m = &e.mem;
                let inv: Vec<String> =
                    (0..30).filter(|&s| m.u8(0x8a95 + s * 6) != 0).map(|s| m.i16(0x8a97 + s * 6).to_string()).collect();
                format!(
                    "t={now} level={} action={}/{} player=({},{}) verb={} text={} bar=\"{}\" held={} inv={}",
                    m.u8(ds::LEVEL),
                    m.u8(ds::CUR_ACTION),
                    m.u8(ds::CUR_ACTION_PC),
                    m.i16(0xfd3c),
                    m.i16(0xfd3e),
                    m.u8(0xfd2c),
                    m.i16(0xfca9),
                    crate::cp850_to_string(&m.cstr(0x8a3c, 80)),
                    m.u8(0x8a8c),
                    inv.join(",")
                )
            })
            .unwrap_or_default()
    }

    pub fn run(&mut self, script: &str) -> Result<(), Error> {
        for cmd in script.split([';', '\n']) {
            let cmd = cmd.split('#').next().unwrap().trim();
            if !cmd.is_empty() {
                self.exec(cmd)?;
            }
        }
        Ok(())
    }

    pub fn exec(&mut self, cmd: &str) -> Result<(), Error> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let n = |i: usize| -> Result<i32, Error> {
            parts
                .get(i)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| Error::new(format!("argumento inválido en '{cmd}'")))
        };
        match parts[0] {
            "wait" => self.step(n(1)? as f64)?,
            "waitidle" => self.wait_idle()?,
            "shot" => {
                if let (Some(f), Some(cb)) = (&self.last, &mut self.on_shot) {
                    cb(parts.get(1).copied().unwrap_or("shot"), f);
                }
            }
            "move" => self.mv(n(1)?, n(2)?, 50.0)?,
            "click" => self.click(n(1)?, n(2)?)?,
            "rclick" => self.rclick()?,
            "type" => {
                for c in parts[1..].join(" ").chars() {
                    self.input(Input::KeyDown(0, Some(crate::engine::Key::Char(c))));
                    self.step(50.0)?;
                }
            }
            "enter" => {
                self.input(Input::KeyDown(28, Some(crate::engine::Key::Enter)));
                self.step(60.0)?;
                self.input(Input::KeyUp(28));
                self.step(40.0)?;
            }
            "key" => {
                self.input(Input::KeyDown(n(1)? as u8, None));
                self.step(60.0)?;
                self.input(Input::KeyUp(n(1)? as u8));
                self.step(40.0)?;
            }
            "verb" => self.set_verb(n(1)?)?,
            "do" => {
                self.mv(n(2)?, n(3)?, 100.0)?;
                self.set_verb(n(1)?)?;
                self.click(n(2)?, n(3)?)?;
                self.wait_idle()?;
            }
            "use" => {
                self.pick_up(n(1)?)?;
                self.mv(n(2)?, n(3)?, 300.0)?;
                self.click(n(2)?, n(3)?)?;
                self.wait_idle()?;
            }
            "combine" => {
                self.pick_up(n(1)?)?;
                let s = self.slot_of(n(2)?).ok_or_else(|| Error::new("objeto no encontrado"))?;
                let x = (s as i32 - self.u8(0xfccc)) * 37 + 30;
                self.mv(x, 15, 200.0)?;
                self.click(x, 15)?;
                self.step(100.0)?;
                self.mv(160, 100, 200.0)?;
                self.wait_idle()?;
            }
            "level" => {
                let (l, d) = (n(1)?, n(2).unwrap_or(0));
                self.handle.with(move |e| {
                    e.mem.set8(0xfcd1, d);
                    e.mem.set8(ds::LEVEL, l);
                });
                self.step(3000.0)?;
            }
            "save" => {
                let (slot, name) = (n(1)? as usize, parts.get(2).unwrap_or(&"test").as_bytes().to_vec());
                self.handle.with(move |e| e.save_state(slot, &name));
            }
            "load" => {
                let slot = n(1)? as usize;
                if self.handle.with(move |e| e.load_state(slot)) != Some(true) {
                    return Err(Error::new("no se pudo cargar la partida"));
                }
                self.step(200.0)?;
            }
            "state" => {
                let s = self.state();
                (self.log)(&s);
            }
            "expect" => {
                let ok = match parts.get(1).copied() {
                    Some("inv") => self.slot_of(n(2)?).is_some(),
                    Some("noinv") => self.slot_of(n(2)?).is_none(),
                    Some("level") => self.u8(ds::LEVEL) == n(2)?,
                    Some("var") => self.var(n(2)?) == n(3)?,
                    Some("exited") => self.exited(),
                    _ => return Err(Error::new(format!("expect desconocido: {cmd}"))),
                };
                if !ok {
                    return Err(Error::new(format!("falló: {cmd}\n  {}", self.state())));
                }
                (self.log)(&format!("ok: {cmd}"));
            }
            other => return Err(Error::new(format!("comando desconocido {other}"))),
        }
        Ok(())
    }
}
