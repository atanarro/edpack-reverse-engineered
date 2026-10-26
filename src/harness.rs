//! Scriptable headless driver (tests and `edtool run`).
//!
//! Script commands, separated by `;` or newlines (`#` starts a comment):
//! ```text
//! wait <ms> | waitidle | shot <name> | state | look | move <x> <y> | click <x> <y> | rclick | choose <n>
//! key <scancode> | type <text> | enter | verb <n> | do <verb> <x> <y> | use <obj> <x> <y> | combine <a> <b>
//! level <n> [door] | save <slot> <name> | load <slot>
//! expect inv|noinv <obj> | expect level <n> | expect var <n> <value> | expect exited
//! ```
//!
//! Wherever a command takes `<x> <y>` it also accepts the name of a thing on
//! screen (`do 3 llave verde`, or `@n` for thing n), resolved to a point where
//! the cursor selects that thing; `<obj>` accepts an inventory object's name instead of its
//! number. Names match case-insensitively, ignoring accents' absence.

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
    /// A harness that starts at level 0 (no shareware notice).
    pub fn new(game: Game) -> Result<Harness, Error> {
        Harness::with_options(game, Options { shareware_notice: false, ..Options::default() })
    }

    pub fn with_options(game: Game, opts: Options) -> Result<Harness, Error> {
        let engine = Engine::new(game, Box::new(MemStorage::default()), opts)?;
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
                // A dialogue choice on screen waits for the player: idle.
                if m.i16(0xfcf0) != -1 && m.u8(0x1bbd) != 0 && m.i16(0xfca9) == 0 {
                    return false;
                }
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
        while s < self.u8(0xfccc) as usize {
            self.click(5, 15)?;
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

    /// Hoverable things on screen: (index, name, rect).
    pub fn things(&mut self) -> Vec<(usize, String, [i32; 4])> {
        self.handle
            .with(|e| {
                let m = &e.mem;
                (0..30)
                    .filter_map(|k| {
                        let t = m.i8(ds::THING_ORDER + k);
                        if !(0..30).contains(&t) {
                            return None;
                        }
                        let a = ds::THINGS + t as usize * crate::memory::THING_SIZE;
                        let md = m.u8(a + 0x2b);
                        (md == 1 || md == 3).then(|| {
                            let name = crate::cp850_to_string(&e.thing_name(t));
                            (t as usize, name, [m.i16(a), m.i16(a + 2), m.i16(a + 4), m.i16(a + 6)])
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A screen point where the cursor selects the thing called `name`.
    fn thing_point(&mut self, name: &str) -> Result<(i32, i32), Error> {
        let things = self.things();
        let want = fold(name);
        let index = name.strip_prefix('@').and_then(|n| n.parse::<usize>().ok());
        let Some(&(t, _, _)) = things
            .iter()
            .find(|(t, n, _)| index.map_or(fold(n) == want, |i| *t == i))
            .or_else(|| things.iter().find(|(_, n, _)| fold(n).contains(&want)))
        else {
            let names: Vec<&str> = things.iter().map(|(_, n, _)| n.as_str()).collect();
            return Err(Error::new(format!("no hay ninguna cosa '{name}' en pantalla ({})", names.join(", "))));
        };
        // Same test as the original's hover: the first thing in draw order
        // whose rectangle contains the hotspot wins.
        let pick = |x: i32, y: i32| {
            things.iter().find(|(_, _, r)| r[0] <= x && x <= r[0] + r[2] && r[1] <= y && y <= r[1] + r[3]).map(|t| t.0)
        };
        let r = things.iter().find(|x| x.0 == t).unwrap().2;
        let (cx, cy) = (r[0] + r[2] / 2, r[1] + r[3] / 2);
        let mut best: Option<(i32, i32, i32)> = None;
        for y in r[1].max(0)..=(r[1] + r[3]).min(199) {
            for x in r[0].max(0)..=(r[0] + r[2]).min(319) {
                if pick(x, y) == Some(t) {
                    let d = (x - cx).abs() + (y - cy).abs();
                    if best.is_none_or(|b| d < b.2) {
                        best = Some((x, y, d));
                    }
                }
            }
        }
        best.map(|b| (b.0, b.1)).ok_or_else(|| Error::new(format!("'{name}' está tapada por otras cosas")))
    }

    /// Inventory object number by number or name.
    fn object(&mut self, arg: &str) -> Result<i32, Error> {
        if let Ok(n) = arg.parse() {
            return Ok(n);
        }
        let want = fold(arg);
        let held: Vec<i32> = self
            .handle
            .with(|e| (0..30).filter(|&s| e.mem.u8(0x8a95 + s * 6) != 0).map(|s| e.mem.i16(0x8a97 + s * 6)).collect())
            .unwrap_or_default();
        let names: Vec<(i32, String)> = held
            .into_iter()
            .map(|o| (o, self.handle.with(move |e| crate::cp850_to_string(&e.object_name(o))).unwrap_or_default()))
            .collect();
        names
            .iter()
            .find(|(_, n)| fold(n) == want)
            .or_else(|| names.iter().find(|(_, n)| fold(n).contains(&want)))
            .map(|o| o.0)
            .ok_or_else(|| {
                let l: Vec<&str> = names.iter().map(|(_, n)| n.as_str()).collect();
                Error::new(format!("no llevo ningún objeto '{arg}' ({})", l.join(", ")))
            })
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

    /// `<x> <y>` or a thing name in `parts[from..]`.
    fn point(&mut self, parts: &[&str], from: usize) -> Result<(i32, i32), Error> {
        let rest = &parts[from.min(parts.len())..];
        if let [x, y] = rest
            && let (Ok(x), Ok(y)) = (x.parse(), y.parse())
        {
            return Ok((x, y));
        }
        if rest.is_empty() {
            return Err(Error::new(format!("falta el destino en '{}'", parts.join(" "))));
        }
        self.thing_point(&rest.join(" "))
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
            "move" => {
                let (x, y) = self.point(&parts, 1)?;
                self.mv(x, y, 50.0)?
            }
            "click" => {
                let (x, y) = self.point(&parts, 1)?;
                self.click(x, y)?
            }
            "look" => {
                let mut out = Vec::new();
                for (t, name, r) in self.things() {
                    let acts = self
                        .handle
                        .with(move |e| {
                            let a = ds::THINGS + t * crate::memory::THING_SIZE + 0x48;
                            (0..5).map(|k| e.mem.i16(a + 2 * k)).collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    out.push(format!("  cosa {t} '{name}' {r:?} acciones {acts:?}"));
                }
                let inv: Vec<i32> = self
                    .handle
                    .with(|e| {
                        (0..30).filter(|&s| e.mem.u8(0x8a95 + s * 6) != 0).map(|s| e.mem.i16(0x8a97 + s * 6)).collect()
                    })
                    .unwrap_or_default();
                for o in inv {
                    let name = self.handle.with(move |e| crate::cp850_to_string(&e.object_name(o))).unwrap_or_default();
                    out.push(format!("  objeto {o} '{name}'"));
                }
                let msg = format!("{}\n{}", self.state(), out.join("\n"));
                (self.log)(&msg);
            }
            "rclick" => self.rclick()?,
            "choose" => {
                // choose <n>: the n-th line (1..5, as defined in SELECTEXT) of the dialogue on screen
                let row = n(1)? - 1;
                let y = self
                    .handle
                    .with(move |e| {
                        let m = &e.mem;
                        let visible = m.i16(0xfcf0) != -1 && (0..5).contains(&row) && m.u8(0xfbd8 + row as usize) == 0;
                        visible.then(|| m.i16(0xfd05) + m.i16(0xfbce + row as usize * 2) + 5)
                    })
                    .flatten()
                    .ok_or_else(|| Error::new(format!("no hay opción {} en pantalla", row + 1)))?;
                self.mv(160, y, 100.0)?;
                self.click(160, y)?;
                self.wait_idle()?;
            }
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
                let (x, y) = self.point(&parts, 2)?;
                self.mv(x, y, 100.0)?;
                self.set_verb(n(1)?)?;
                self.click(x, y)?;
                self.wait_idle()?;
            }
            "use" => {
                // use <obj> <target>: the object is one word (number or name)
                let obj = self.object(parts.get(1).copied().unwrap_or(""))?;
                let (x, y) = self.point(&parts, 2)?;
                self.pick_up(obj)?;
                // leave the inventory panel first (it stays open while the
                // cursor is in it), then go to the target
                self.mv(x, 100, 200.0)?;
                self.mv(x, y, 300.0)?;
                self.click(x, y)?;
                self.wait_idle()?;
            }
            "combine" => {
                let a = self.object(parts.get(1).copied().unwrap_or(""))?;
                let b = self.object(parts.get(2).copied().unwrap_or(""))?;
                self.pick_up(a)?;
                let s = self.slot_of(b).ok_or_else(|| Error::new("objeto no encontrado"))?;
                let col = s as i32 - self.u8(0xfccc);
                if !(0..8).contains(&col) {
                    return Err(Error::new(format!("el objeto {b} no se ve en el inventario")));
                }
                let x = col * 37 + 30;
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

/// Lower-case, without accents, for name matching.
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'Á' | 'À' => 'a',
            'é' | 'è' | 'É' | 'È' => 'e',
            'í' | 'ì' | 'Í' | 'Ì' => 'i',
            'ó' | 'ò' | 'Ó' | 'Ò' => 'o',
            'ú' | 'ù' | 'ü' | 'Ú' | 'Ù' | 'Ü' => 'u',
            'ñ' | 'Ñ' => 'n',
            c => c.to_ascii_lowercase(),
        })
        .collect::<String>()
        .trim()
        .to_string()
}
