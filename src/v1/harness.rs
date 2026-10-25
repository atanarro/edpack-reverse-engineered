//! Scriptable headless driver for v1 games (tests and `edtool run`).
//! Same command language as [`crate::harness`], with v1 semantics:
//!
//! ```text
//! wait <ms> | waitidle | waitlevel <n> | shot <name> | state | look | move <p> | click <p> | rclick <p> | tap <p> | rtap <p>
//! key <scancode> | type <text> | enter | use <obj> <p> | combine <a> <b> | look_at <p>
//! save <slot> <name> | load <slot> | expect inv|noinv <obj> | expect level <n>
//! expect var <n> <value> | expect exited
//! ```
//!
//! `<p>` is `<x> <y>` or a thing name; `<obj>` an object number or name.
//! v1 has no verbs: `click` on a thing does what its type says (look, use,
//! go...), `rclick` is "take", left click on an inventory object shows its
//! description, right click picks it up.

use crate::Error;
use crate::engine::MemStorage;
use crate::host::{Frame, Input, Key};

use super::data::{THING_SIZE, ds};
use super::engine::{Engine, EngineHandle, Options, g};
use super::player as p;

pub type ShotFn = Box<dyn FnMut(&str, &Frame)>;

pub struct Harness {
    pub handle: EngineHandle,
    pub now: f64,
    pending: Vec<Input>,
    pub last: Option<Frame>,
    pub on_shot: Option<ShotFn>,
    pub log: Box<dyn FnMut(&str)>,
}

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

impl Harness {
    pub fn new(src: super::data::Source) -> Result<Harness, Error> {
        Harness::with_options(src, Options::default())
    }

    pub fn with_options(src: super::data::Source, opts: Options) -> Result<Harness, Error> {
        let engine = Engine::new(src, Box::new(MemStorage::default()), opts)?;
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

    pub fn step(&mut self, ms: f64) -> Result<(), Error> {
        let end = self.now + ms;
        while self.now < end && !self.handle.finished {
            self.now += 16.0;
            let ev = std::mem::take(&mut self.pending);
            self.last = self.handle.advance(self.now, ev);
        }
        match &self.handle.error {
            Some(e) => Err(Error::new(format!("el juego terminó con un error: {e}"))),
            None => Ok(()),
        }
    }

    fn mem<R: Send + 'static>(&mut self, f: impl FnOnce(&Engine) -> R + Send + 'static) -> Option<R> {
        self.handle.with(move |e| f(e))
    }

    pub fn var(&mut self, n: i32) -> i32 {
        self.mem(move |e| e.mem.get_var(n)).unwrap_or(0)
    }

    fn press(&mut self, b: usize, x: i32, y: i32) -> Result<(), Error> {
        self.pending.push(Input::MouseMove(x, y));
        self.step(80.0)?;
        // When the game takes input, wait until it shows the target as
        // hovered: a pass can take a while (the first one in a room redraws
        // everything), and a click it does not see is lost.
        let want = self.expected_hover(x, y);
        let end = self.now + 1500.0;
        while let Some(w) = want
            && self.now < end
            && self.hover() != w
        {
            self.step(20.0)?;
        }
        self.pending.push(Input::Button(b, true));
        // The original polls the buttons once per pass and only counts a
        // press if the button is still held: hold it until the game has
        // taken the press.
        self.step(40.0)?;
        let end = self.now + 1500.0;
        while self.now < end && self.mem(move |e| e.buttons[b].presses).unwrap_or(0) > 0 {
            self.step(20.0)?;
        }
        self.step(60.0)?;
        self.pending.push(Input::Button(b, false));
        self.step(50.0)
    }

    /// (hovered thing + 1, hovered inventory slot + 1) as the game has them.
    fn hover(&mut self) -> (i32, i32) {
        self.mem(|e| (e.mem.u8(0x9202), e.mem.i16(0x9280))).unwrap_or((0, 0))
    }

    /// What the game should report as hovered at (x, y), if it takes input now.
    fn expected_hover(&mut self, x: i32, y: i32) -> Option<(i32, i32)> {
        let (mode, action, speaking, scroll, held, held_obj) = self.mem(|e| {
            let m = &e.mem;
            (
                m.i16(g::CURSOR_STATE),
                m.u8(g::ACTION),
                m.u8(g::TEXT_ACTIVE),
                m.u8(g::INV_SCROLL),
                m.u8(0x35a0),
                m.u8(0x35a2),
            )
        })?;
        if mode != 0 || (action > 0 && action < 50) || speaking != 0 {
            return None;
        }
        let thing = self
            .things()
            .iter()
            .find(|(_, _, r)| r[0] <= x && x <= r[0] + r[2] && r[1] <= y && y <= r[1] + r[3])
            .map_or(0, |t| t.0 as i32 + 1);
        let inv = self.inventory();
        let slot = (0..8)
            .find(|&i| 13 + 37 * i <= x && x <= 48 + 37 * i && y >= 173)
            .and_then(|i| inv.iter().find(|o| o.0 as i32 == scroll + i))
            .filter(|o| held == 0 || o.1 != held_obj)
            .map_or(0, |o| o.0 as i32 + 1);
        Some((thing, slot))
    }

    /// Hoverable things: (index, name, rect).
    pub fn things(&mut self) -> Vec<(usize, String, [i32; 4])> {
        self.mem(|e| {
            let m = &e.mem;
            (0..30)
                .filter_map(|t| {
                    let a = ds::THINGS + t * THING_SIZE;
                    let st = m.i16(a + 0x22);
                    (st == 1 || st == 3).then(|| {
                        let name = crate::cp850_to_string(&m.cstr(a + 0x26, 15));
                        (t, name, [m.i16(a + 0x10), m.i16(a + 0x12), m.i16(a + 0x18), m.i16(a + 0x1a)])
                    })
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// Inventory: (slot, object, name).
    pub fn inventory(&mut self) -> Vec<(usize, i32, String)> {
        self.mem(|e| {
            let m = &e.mem;
            (0..30)
                .filter(|&s| m.u8(g::INV + s * g::INV_SIZE + 0x1e) != 0)
                .map(|s| {
                    let r = g::INV + s * g::INV_SIZE;
                    (s, m.u8(r + 0x20), crate::cp850_to_string(&m.cstr(r, 30)))
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn thing_point(&mut self, name: &str) -> Result<(i32, i32), Error> {
        let things = self.things();
        let want = fold(name);
        let Some(&(t, _, r)) = things
            .iter()
            .find(|(_, n, _)| fold(n) == want)
            .or_else(|| things.iter().find(|(_, n, _)| fold(n).contains(&want)))
        else {
            let names: Vec<&str> = things.iter().map(|(_, n, _)| n.as_str()).collect();
            return Err(Error::new(format!("no hay ninguna cosa '{name}' en pantalla ({})", names.join(", "))));
        };
        let pick = |x: i32, y: i32| {
            things.iter().find(|(_, _, r)| r[0] <= x && x <= r[0] + r[2] && r[1] <= y && y <= r[1] + r[3]).map(|t| t.0)
        };
        let (cx, cy) = (r[0] + r[2] / 2, r[1] + r[3] / 2);
        let mut best: Option<(i32, i32, i32)> = None;
        for y in r[1].max(0)..=(r[1] + r[3]).min(172) {
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

    fn object(&mut self, arg: &str) -> Result<i32, Error> {
        if let Ok(n) = arg.parse() {
            return Ok(n);
        }
        let inv = self.inventory();
        let want = fold(arg);
        inv.iter()
            .find(|(_, _, n)| fold(n) == want)
            .or_else(|| inv.iter().find(|(_, _, n)| fold(n).contains(&want)))
            .map(|o| o.1)
            .ok_or_else(|| {
                let l: Vec<&str> = inv.iter().map(|(_, _, n)| n.as_str()).collect();
                Error::new(format!("no llevo ningún objeto '{arg}' ({})", l.join(", ")))
            })
    }

    /// Click point of inventory object `obj` (scrolling the bar if needed).
    fn object_point(&mut self, obj: i32) -> Result<(i32, i32), Error> {
        let slot = self
            .inventory()
            .iter()
            .find(|o| o.1 == obj)
            .map(|o| o.0)
            .ok_or_else(|| Error::new(format!("el objeto {obj} no está en el inventario")))?;
        for _ in 0..30 {
            let scroll = self.mem(|e| e.mem.u8(g::INV_SCROLL)).unwrap_or(0) as usize;
            if slot < scroll {
                self.press(0, 5, 185)?;
            } else if slot >= scroll + 8 {
                self.press(0, 312, 185)?;
            } else {
                return Ok((13 + 37 * (slot - scroll) as i32 + 17, 185));
            }
        }
        Err(Error::new("no puedo desplazar el inventario"))
    }

    fn busy(&mut self) -> bool {
        self.mem(|e| {
            let m = &e.mem;
            m.u8(g::ACTION) != 0
                || m.u8(g::TEXT_ACTIVE) != 0
                || m.u8(p::WALKING) != 0
                || m.i16(g::LEVEL) != m.i16(g::PREV_LEVEL)
                || m.i16(g::CURSOR_STATE) != 0
        })
        .unwrap_or(false)
    }

    pub fn wait_idle(&mut self) -> Result<(), Error> {
        let end = self.now + 600000.0;
        self.step(100.0)?;
        while !self.handle.finished && self.now < end && self.busy() {
            self.step(50.0)?;
        }
        self.step(200.0)
    }

    /// The state without the clock and the label: level, action, player, texts, inventory.
    pub fn state_core(&mut self) -> String {
        let s = self.state();
        let s = s.split_once(' ').map_or(s.as_str(), |x| x.1).to_string();
        match (s.find(" label="), s.find(" held=")) {
            (Some(a), Some(b)) => format!("{}{}", &s[..a], &s[b..]),
            _ => s,
        }
    }

    pub fn state(&mut self) -> String {
        let now = self.now;
        let inv: Vec<String> = self.inventory().iter().map(|o| format!("{}:{}", o.1, o.2)).collect();
        self.mem(move |e| {
            let m = &e.mem;
            format!(
                "t={now} level={} action={}/{} player=({},{}) text={} label=\"{}\" held={} inv=[{}]",
                m.i16(g::LEVEL),
                m.u8(g::ACTION),
                m.u8(g::ACTION_PC),
                m.i16(p::PX),
                m.i16(p::PY),
                m.u8(g::TEXT_ACTIVE),
                crate::cp850_to_string(&m.cstr(0x3550, 80)),
                m.u8(0x35a0),
                inv.join(", ")
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
            "waitlevel" => {
                // wait (up to 10 minutes) until level n is loaded
                let want = n(1)?;
                let end = self.now + 600000.0;
                while !self.handle.finished && self.now < end {
                    let (l, p) = self.mem(|e| (e.mem.i16(g::LEVEL), e.mem.i16(g::PREV_LEVEL))).unwrap_or((-1, -1));
                    if l == want && p == want {
                        break;
                    }
                    self.step(50.0)?;
                }
                self.step(200.0)?;
            }
            "shot" => {
                if let (Some(f), Some(cb)) = (&self.last, &mut self.on_shot) {
                    cb(parts.get(1).copied().unwrap_or("shot"), f);
                }
            }
            "move" => {
                let (x, y) = self.point(&parts, 1)?;
                self.pending.push(Input::MouseMove(x, y));
                self.step(50.0)?;
            }
            "tap" | "rtap" => {
                // a click that does not wait for the game to settle (intros, cut-scenes)
                let (x, y) = self.point(&parts, 1)?;
                self.press((parts[0] == "rtap") as usize, x, y)?;
            }
            "click" | "rclick" => {
                let (x, y) = self.point(&parts, 1)?;
                self.press((parts[0] == "rclick") as usize, x, y)?;
                self.wait_idle()?;
            }
            "look_at" => {
                let obj = self.object(parts.get(1).copied().unwrap_or(""))?;
                let (x, y) = self.object_point(obj)?;
                self.press(0, x, y)?;
                self.wait_idle()?;
            }
            "use" => {
                let obj = self.object(parts.get(1).copied().unwrap_or(""))?;
                let (ox, oy) = self.object_point(obj)?;
                self.press(1, ox, oy)?;
                let (x, y) = if let Ok(o2) = self.object(&parts[2..].join(" ")) {
                    self.object_point(o2)?
                } else {
                    self.point(&parts, 2)?
                };
                self.press(0, x, y)?;
                self.wait_idle()?;
            }
            "combine" => {
                let a = self.object(parts.get(1).copied().unwrap_or(""))?;
                let b = self.object(parts.get(2).copied().unwrap_or(""))?;
                let (ax, ay) = self.object_point(a)?;
                self.press(1, ax, ay)?;
                let (bx, by) = self.object_point(b)?;
                self.press(0, bx, by)?;
                self.wait_idle()?;
            }
            "key" => {
                let k = n(1)? as u8;
                self.pending.push(Input::KeyDown(k, None));
                self.step(60.0)?;
                self.pending.push(Input::KeyUp(k));
                self.step(40.0)?;
            }
            "type" => {
                for c in parts[1..].join(" ").chars() {
                    self.pending.push(Input::KeyDown(0, Some(Key::Char(c))));
                    self.step(50.0)?;
                }
            }
            "enter" => {
                self.pending.push(Input::KeyDown(28, Some(Key::Enter)));
                self.step(60.0)?;
                self.pending.push(Input::KeyUp(28));
                self.step(40.0)?;
            }
            "save" => {
                let (slot, name) = (n(1)? as usize, parts.get(2).unwrap_or(&"test").as_bytes().to_vec());
                self.handle.with(move |e| e.save_state(slot, &name));
            }
            "load" => {
                // loading fades the screen, so it runs on the engine's own
                // loop: request it and let time pass
                let slot = n(1)? as usize;
                let ok = self.handle.with(move |e| {
                    let saved = e.save_names().get(slot).is_some_and(|n| n.is_some());
                    if saved {
                        e.pending_load = Some(slot);
                    }
                    saved
                });
                if ok != Some(true) {
                    return Err(Error::new("no se pudo cargar la partida"));
                }
                self.step(3000.0)?;
            }
            "state" => {
                let s = self.state();
                (self.log)(&s);
            }
            "look" => {
                let mut out = vec![self.state()];
                for (t, name, r) in self.things() {
                    out.push(format!("  cosa {t} '{name}' {r:?}"));
                }
                let msg = out.join("\n");
                (self.log)(&msg);
            }
            "expect" => {
                let ok = match parts.get(1).copied() {
                    Some("inv") | Some("noinv") => {
                        let arg = parts[2..].join(" ");
                        let held = match self.object(&arg) {
                            Ok(o) => self.inventory().iter().any(|i| i.1 == o),
                            Err(_) => false,
                        };
                        held == (parts[1] == "inv")
                    }
                    Some("level") => self.mem(|e| e.mem.i16(g::LEVEL)).unwrap_or(-1) == n(2)?,
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
