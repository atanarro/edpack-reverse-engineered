//! The RUNVGA v1.03 interpreter: engine state, start-up, level changes, the
//! bytecode interpreter (0x2464) and its helpers.
//! See docs/v1-runvga/.

use std::sync::{Arc, Mutex};

use crate::Error;
use crate::audio::Mixer;
use crate::audio::voc::{Voice, parse_voc};
use crate::engine::Storage;
use crate::host::{self, Core, Fatal, Frame, Host, Input, Key};
use crate::memory::{Memory, SCRATCH_VAR_ADDR};

use super::data::{self, Font, Source, THING_SIZE, ds};
use super::player as p;

/// The timer runs at [0x97] = 50 Hz.
pub const TICK_MS: f64 = 20.0;
/// A retrace of a 70.086 Hz VGA.
pub const RETRACE_MS: f64 = 1000.0 / 70.086;

/// Global DS addresses (see the v1 specs).
pub mod g {
    pub const LEVEL: usize = 0x927c;
    pub const PREV_LEVEL: usize = 0x927e;
    pub const DOOR: usize = 0x92a3;
    pub const ACTION: usize = 0x929c;
    pub const ACTION_PC: usize = 0x929d;
    pub const ACTION_FLAG: usize = 0x929e;
    pub const TEXT_ACTIVE: usize = 0x9282;
    pub const TEXT_QUEUE: usize = 0x9283;
    pub const TEXT_INDEX: usize = 0x9298;
    pub const TEXT_BY_PLAYER: usize = 0x9297;
    pub const TEXT_SHOWN: usize = 0x9299;
    pub const PTEXT_X: usize = 0x929a;
    pub const PTEXT_Y: usize = 0x929b;
    pub const PTEXT_COLOUR: usize = 0x92a7;
    pub const SPEECH_COLOUR: usize = 0x92a5;
    pub const SPEECH_BORDER: usize = 0x92a6;
    pub const PAL_DIRTY: usize = 0x92a4;
    pub const ANIM_WAIT: usize = 0x92a2;
    pub const ANIM_TICKS: usize = 0x3537;
    pub const TEXT_TICKS: usize = 0x3538;
    pub const COUNTDOWN: usize = 0x92a8;
    pub const PAUSED: usize = 0x95;
    pub const KEY: usize = 0x96;
    pub const CURSOR_STATE: usize = 0x92b0;
    pub const MOUSE_X: usize = 0x92b4;
    pub const MOUSE_Y: usize = 0x92b6;
    pub const CURSOR_SRCX: usize = 0x92ac;
    pub const CURSOR_SRCY: usize = 0x92ae;
    pub const CURSOR_W: usize = 0x92bc;
    pub const CURSOR_H: usize = 0x92be;
    pub const HOT_X: usize = 0x92c4;
    pub const HOT_Y: usize = 0x92c6;
    pub const MUSIC: usize = 0x984;
    pub const MUSIC_ON: usize = 0xddf1;
    pub const INV_SCROLL: usize = 0x929f;
    pub const HOVER: usize = 0x9202;
    /// Inventory slots: 30 x 0x22 {name[30], used, changed, obj, ...}.
    pub const INV: usize = 0x35a9;
    pub const INV_SIZE: usize = 0x22;
    /// Inventory descriptions, 30 x 250 (text slots 50..79).
    pub const INV_TEXTS: usize = 0x728f;
    /// Palettes: scene (6-bit), full-screen picture, and the four slots (8-bit).
    pub const PAL_SCENE: usize = 0x9924;
    pub const PAL_PICTURE: usize = 0x9c24;
    pub const PAL_SLOT: [usize; 4] = [0x9f24, 0xa224, 0xa824, 0xa524];
    pub const USER_VARS_PTR: usize = 0x0ea0;
}

/// SETUP.CFG speed -> (ticks per walk step [0x3536], ticks between animation runs [0x3537]).
const SETUP_SPEED: [(i32, i32); 5] = [(15, 12), (8, 6), (4, 3), (3, 2), (1, 0)];
/// SETUP.CFG text speed -> [0x3538].
const SETUP_TEXT: [i32; 5] = [16, 12, 8, 4, 2];

fn message(code: u32) -> &'static str {
    match code {
        1 => "No encuentro el fichero del INVENTARIO.",
        2 => "No encuentro el fichero de las LETRAS.",
        6 => "El archivo de graficos '%s' no es del formato 320*200 256 colores.",
        0x62 => "Fichero '%s' no es un PCX.",
        0x63 => "Error al intentar leer el archivo '%s' dentro de RESOURCE.DAT.",
        0x65 => "No encuentro el nivel '%s'.",
        0x66 => "No encuentro archivo de graficos(1) '%s'.",
        0x67 => "No encuentro archivo de graficos(0) '%s'.",
        0x68 => "No encuentro archivo '%s'.",
        0x69 => "Nivel de comienzo no especificado en el Nivel 0",
        0x6a => "Fichero de los MOVIMIENTOS no especificado en el nivel 0",
        0x6b => "Fichero del INVENTARIO no especificado en el nivel 0",
        0x6c => "No encuentro archivo de graficos(2) '%s'.",
        0x6d => "No encuentro archivo de graficos(3) '%s'.",
        0x6e => "No encuentro archivo de graficos '%s'.",
        0x6f => "Commando no valida.",
        _ => "Error interno %s",
    }
}

/// Script variable n -> DS address (the far-pointer table at 0x0ea4).
fn var_table() -> Vec<usize> {
    const GLOBALS: [usize; 13] =
        [0x91f5, 0x91da, 0x91dd, 0x91df, 0x91e9, 0x91eb, 0x91f7, 0x9282, 0x92a5, 0x92a6, 0x92b0, 0x3539, 0x353b];
    const THING_FIELDS: [usize; 13] = [0x22, 0x0c, 0x3f, 0x41, 0x3d, 0x10, 0x12, 0x24, 0x35, 0x37, 0x39, 0x3b, 0x43];
    let mut v = vec![SCRATCH_VAR_ADDR; 2000];
    v[..13].copy_from_slice(&GLOBALS);
    for t in 0..25 {
        for (k, &off) in THING_FIELDS.iter().enumerate() {
            v[30 + 20 * t + k] = ds::THINGS + t * THING_SIZE + off;
        }
    }
    v
}

pub struct Options {
    pub speed: usize,
    pub text_speed: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { speed: 2, text_speed: 3 }
    }
}

impl Options {
    /// The settings of a SETUP.CFG (byte 1 speed, byte 2 text speed), as
    /// RUNVGA reads them; `game` is the archive or the game folder.
    pub fn from_setup_cfg(game: &std::path::Path) -> Option<Options> {
        let dir = if game.is_dir() { game } else { game.parent()? };
        let cfg = std::fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().eq_ignore_ascii_case("SETUP.CFG"))?;
        let b = std::fs::read(cfg.path()).ok()?;
        Some(Options { speed: *b.get(1)? as usize, text_speed: *b.get(2)? as usize })
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct ButtonState {
    pub held: bool,
    pub presses: u32,
}

pub struct Engine {
    pub mem: Memory,
    pub code: Vec<i16>,
    pub src: Source,
    /// LIMIT record size of the RUNVGA build the game was made for
    pub limit_stride: usize,
    pub font: Font,
    /// gfx slots 0..3: background, things, player/cursor sheet, inventory icons
    pub sheets: [Vec<u8>; 4],
    /// sprites, texts and icons; 0 = transparent
    pub overlay: Vec<u8>,
    /// what the screen shows (without the cursor)
    pub vga: Vec<u8>,
    pub screen: Vec<u8>,
    pub dac: [u8; 768],
    pub palette: [u8; 768],
    pub exited: bool,
    pub(super) host: Host<Engine>,
    pub(super) buttons: [ButtonState; 2],
    pub(super) pointer: (i32, i32),
    /// key state per scan code (the int 9 handler's table at 0x98a4)
    pub(super) keys: [u8; 128],
    /// savegames are stored as "<save_prefix>/SAVEGAME._N_"
    pub save_prefix: String,
    /// a load requested from outside the engine thread (tests, tools)
    pub(super) pending_load: Option<usize>,
    pub last_char: Option<Key>,
    voice: Option<Voice>,
    pub mixer: Arc<Mutex<Mixer>>,
    pub(super) storage: Box<dyn Storage>,
}

impl Engine {
    pub fn new(src: Source, storage: Box<dyn Storage>, opts: Options) -> Result<Engine, Error> {
        let font = match src.entry(998) {
            Some((b, off)) if off < b.len() => Font::new(&b[off..(off + data::FONT_SIZE).min(b.len())]),
            _ => return Err(Error::new(message(2))),
        };
        if src.entry(999).is_none() {
            return Err(Error::new(message(1)));
        }
        let limit_stride = src.version().limit_stride();
        let mut e = Engine {
            limit_stride,
            mem: Memory::with_vars(var_table()),
            code: vec![0; 5000],
            src,
            font,
            sheets: std::array::from_fn(|_| vec![0; 64000]),
            overlay: vec![0; 64000],
            vga: vec![0; 64000],
            screen: vec![0; 64000],
            dac: [0; 768],
            palette: [0; 768],
            exited: false,
            host: Host::default(),
            buttons: [ButtonState::default(); 2],
            pointer: (160, 100),
            keys: [0; 128],
            save_prefix: "edpack-v1".into(),
            pending_load: None,
            last_char: None,
            voice: None,
            mixer: Arc::new(Mutex::new(Mixer::new(44100.0))),
            storage,
        };
        e.init_globals(opts);
        Ok(e)
    }

    /// main() 0xbc17: defaults before the first level.
    fn init_globals(&mut self, opts: Options) {
        let m = &mut self.mem;
        m.set8(0x97, 50);
        let (step, anim) = SETUP_SPEED[opts.speed.min(4)];
        m.set8(p::STEP_TICKS, step);
        m.set8(g::ANIM_TICKS, anim);
        m.set8(g::TEXT_TICKS, SETUP_TEXT[opts.text_speed.min(4)]);
        m.set16(g::MUSIC, -1);
        m.set8(p::STEP_X, 10);
        m.set8(p::STEP_Y, 4);
        m.set16(p::FRAME_W, 32);
        m.set16(p::FRAME_H, 50);
        m.set8(g::SPEECH_COLOUR, 1);
        m.set8(g::SPEECH_BORDER, 15);
        m.set16(g::LEVEL, 0);
        m.set16(g::PREV_LEVEL, -1);
        m.set16(p::PATH, -100);
        m.set16(p::PATH + 2, -100);
        m.set8(g::INV_SCROLL, 0);
        m.set16(0x3539, 2);
        m.set16(0x353b, 15);
        m.set16(g::CURSOR_STATE, 3);
        m.set16(g::MOUSE_X, 100);
        m.set16(g::MOUSE_Y, 100);
    }

    // ------------------------------------------------------------ time, errors

    pub fn wait(&mut self, ms: f64) {
        host::wait(self, ms)
    }

    pub fn clock(&self) -> f64 {
        self.host.clock
    }

    pub fn fatal(&self, code: u32, arg: &str) -> ! {
        std::panic::panic_any(Fatal(message(code).replace("%s", arg)))
    }

    // ------------------------------------------------------------ variables

    pub fn var(&self, n: i32) -> i32 {
        self.mem.get_var(n)
    }

    pub fn set_var(&mut self, n: i32, v: i32) {
        self.mem.set_var(n, v)
    }

    // ------------------------------------------------------------ interpreter

    /// run(base, action, pc, flag) 0x2464: 0 = finished, 1 = yield.
    pub fn run_script(&mut self, base: i32, action_addr: usize, pc_addr: usize, flag_addr: usize) -> i32 {
        loop {
            let pc = self.mem.u8(pc_addr);
            let word = |e: &Engine, k: i32| -> i32 {
                let i = base + pc + k;
                if (0..5000).contains(&i) { e.code[i as usize] as i32 } else { 0 }
            };
            let op = word(self, 0);
            if op == 0 {
                return 0;
            }
            let raw: [i32; 5] = std::array::from_fn(|k| word(self, k as i32 + 1));
            let a: [i32; 5] = std::array::from_fn(|k| if raw[k] >= 25000 { self.var(raw[k] - 25000) } else { raw[k] });
            let next = |e: &mut Engine, k: i32| e.mem.set8(pc_addr, pc + k);
            let jump = |e: &mut Engine, cond: bool| {
                if cond { e.mem.set8(pc_addr, raw[0] & 0xff) } else { e.mem.set8(pc_addr, pc + 2) }
            };
            let flag = self.mem.u8(flag_addr);
            match op {
                1 => {
                    self.mem.set16(g::LEVEL, a[0]);
                    self.mem.set8(g::DOOR, a[1]);
                    next(self, 3);
                    return 1;
                }
                2 => {
                    p::go_xy(&mut self.mem, a[0], a[1], self.limit_stride);
                    next(self, 3);
                    return 1;
                }
                3 => {
                    if self.mem.u8(g::TEXT_ACTIVE) == 0 {
                        self.say(&[a[0]]);
                        next(self, 2);
                    }
                    return 1;
                }
                4 => {
                    if self.mem.u8(g::TEXT_ACTIVE) == 0 {
                        self.ptext(a[0], a[1], a[2], a[3]);
                        next(self, 5);
                    }
                    return 1;
                }
                7..=12 => {
                    let src = if op % 2 == 0 { self.var(raw[1]) } else { raw[1] };
                    let cur = self.var(raw[0]);
                    let v = match op {
                        7 | 8 => src,
                        9 | 10 => cur + src,
                        _ => cur - src,
                    };
                    self.set_var(raw[0], v);
                    next(self, 3);
                }
                13 | 14 => {
                    let at = (ds::ANIMS as i32 + a[0] * 10) as usize;
                    self.mem.set16(at, (op == 13) as i32);
                    next(self, 2);
                }
                15 => {
                    next(self, 1);
                    return 1;
                }
                16 => self.mem.set8(pc_addr, raw[0] & 0xff),
                17 | 18 => {
                    let x = self.var(raw[0]) as i16 as i32;
                    let y = if op == 17 { raw[1] } else { self.var(raw[1]) } as i16 as i32;
                    self.mem.set8(
                        flag_addr,
                        if x == y {
                            1
                        } else if x < y {
                            2
                        } else {
                            3
                        },
                    );
                    next(self, 3);
                }
                19 => jump(self, flag == 1),
                20 => jump(self, flag != 1),
                21 => jump(self, flag == 2),
                22 => jump(self, flag == 3),
                23 => jump(self, flag == 2 || flag == 1),
                24 => jump(self, flag == 3 || flag == 1),
                25 => {
                    self.add_inv(a[0]);
                    next(self, 2);
                }
                26 => {
                    self.sub_inv(a[0]);
                    next(self, 2);
                }
                27..=30 => {
                    self.palette_op(op, a[0], a[1], a[2]);
                    self.mem.set8(g::PAL_DIRTY, 1);
                    next(self, if op <= 28 { 4 } else { 3 });
                }
                31 => {
                    let t = ds::THINGS as i32 + a[0] * THING_SIZE as i32;
                    let c = ds::COORANIM as i32 + a[1] * 8;
                    if t >= 0 && c >= 0 {
                        let (t, c) = (t as usize, c as usize);
                        for (dst, src) in [(0, 0), (2, 2), (0x18, 4), (0x1a, 6)] {
                            let v = self.mem.i16(c + src);
                            self.mem.set16(t + dst, v);
                        }
                    }
                    next(self, 3);
                }
                32 => {
                    // the pc moves on only after the picture (0x31c5)
                    self.show_pcx(a[0], a[1], a[2]);
                    next(self, 4);
                }
                33 => {
                    self.load_pcx(a[0], a[1]);
                    next(self, 3);
                }
                34 => {
                    self.full_redraw();
                    self.present_all();
                    next(self, 1);
                }
                35 => {
                    if a[0] >= 0 {
                        self.mem.set8(p::STEP_X, a[0]);
                    }
                    if a[1] >= 0 {
                        self.mem.set8(p::STEP_Y, a[1]);
                    }
                    next(self, 3);
                }
                36 => {
                    self.play_cmf(a[0]);
                    next(self, 2);
                }
                37 => {
                    self.mem.set8(g::MUSIC_ON, 0);
                    self.mixer.lock().unwrap().stop_music();
                    self.mem.set16(g::MUSIC, -1);
                    next(self, 1);
                }
                38 => {
                    if let Some(v) = self.voice.clone() {
                        self.mixer.lock().unwrap().play_voice(v);
                    }
                    next(self, 1);
                }
                39 => {
                    p::face(&mut self.mem, a[0]);
                    next(self, 2);
                }
                40 => {
                    let bytes = self.file_bytes(a[0]).unwrap_or_else(|n| self.fatal(0x68, &n));
                    self.voice = parse_voc(&bytes).ok();
                    next(self, 2);
                }
                41 => {
                    self.fade_out();
                    self.exited = true;
                    std::panic::panic_any(host::Exit);
                }
                _ => {
                    self.mem.set8(action_addr, 0);
                    self.fatal(0x6f, "");
                }
            }
        }
    }

    // ------------------------------------------------------------ text requests

    /// say(t0..) 0x73f5: queue texts spoken by the player.
    pub fn say(&mut self, texts: &[i32]) {
        let m = &mut self.mem;
        for k in 0..10 {
            m.set16(g::TEXT_QUEUE + 2 * k, texts.get(k).copied().unwrap_or(-1));
        }
        m.set8(g::TEXT_INDEX, 0);
        m.set8(g::TEXT_ACTIVE, 1);
        m.set8(g::TEXT_BY_PLAYER, 1);
        m.set8(g::TEXT_SHOWN, 0xff);
        let f = m.u8(p::FRAME);
        for (s, c) in [(p::S_SIDE, p::C_SIDE), (p::S_BACK, p::C_BACK), (p::S_FRONT, p::C_FRONT)] {
            let (start, len) = (m.u8(s), m.i8(c));
            if start < f && f < start + len - 1 {
                m.set8(p::FRAME, start);
                break;
            }
        }
        let f = m.u8(p::FRAME);
        m.set8(p::SAVED, f);
        m.set8(p::FRAME, 0);
    }

    /// ptext(x, y, text, colour) 0x74c4.
    pub fn ptext(&mut self, x: i32, y: i32, text: i32, colour: i32) {
        let m = &mut self.mem;
        m.set8(g::PTEXT_X, x);
        m.set8(g::PTEXT_Y, y);
        m.set8(g::PTEXT_COLOUR, colour);
        for k in 0..6 {
            m.set16(g::TEXT_QUEUE + 2 * k, if k == 0 { text } else { -1 });
        }
        m.set8(g::TEXT_INDEX, 0);
        m.set8(g::TEXT_ACTIVE, 1);
        m.set8(g::TEXT_BY_PLAYER, 0);
        m.set8(g::TEXT_SHOWN, 0xff);
    }

    // ------------------------------------------------------------ inventory

    /// addinv(obj) 0x689a.
    pub fn add_inv(&mut self, obj: i32) {
        let Some(s) = (0..30).find(|&s| self.mem.u8(g::INV + s * g::INV_SIZE + 0x1e) == 0) else { return };
        let Some((text, name)) = data::inventory_record(&self.src, obj) else { self.fatal(1, "") };
        let r = g::INV + s * g::INV_SIZE;
        let m = &mut self.mem;
        m.set8(r + 0x1e, 1);
        m.set8(r + 0x1f, 0);
        m.set8(r + 0x20, obj);
        let mut n = name;
        n.truncate(29);
        n.push(0);
        m.fill(r, 30, 0);
        m.write(r, &n);
        let mut t = text;
        t.truncate(249);
        t.push(0);
        m.fill(g::INV_TEXTS + s * 250, 250, 0);
        m.write(g::INV_TEXTS + s * 250, &t);
    }

    /// subinv(obj) 0x6ba3, with the original's compaction bug: it shifts the
    /// level texts (0x41bb) instead of the inventory descriptions.
    pub fn sub_inv(&mut self, obj: i32) {
        let m = &mut self.mem;
        let Some(mut s) = (0..30).find(|&s| m.u8(g::INV + s * g::INV_SIZE + 0x20) == obj & 0xff) else { return };
        let r = g::INV + s * g::INV_SIZE;
        m.set8(r + 0x1e, 0);
        m.set8(r + 0x20, 0);
        while s + 1 < 30 && m.u8(g::INV + (s + 1) * g::INV_SIZE + 0x1e) != 0 {
            let (a, b) = (g::INV + s * g::INV_SIZE, g::INV + (s + 1) * g::INV_SIZE);
            for off in [0x1e, 0x20] {
                let v = m.u8(b + off);
                m.set8(a + off, v);
            }
            let name = m.cstr(b, 30);
            m.write(a, &[name.as_slice(), &[0]].concat());
            let text = m.cstr(ds::TEXTS + (s + 1) * 250, 250);
            m.write(ds::TEXTS + s * 250, &[text.as_slice(), &[0]].concat());
            m.set8(b + 0x1e, 0);
            m.set8(b + 0x20, 0);
            s += 1;
        }
    }

    // ------------------------------------------------------------ palettes

    fn palette_op(&mut self, op: i32, first: i32, count: i32, amount: i32) {
        let first = first.clamp(0, 255) as usize;
        let mut count = count.max(0) as usize;
        if first + count > 255 {
            count = 256 - first;
        }
        if count == 0 {
            return;
        }
        let m = &mut self.mem;
        let base = g::PAL_SCENE + first * 3;
        match op {
            27 => {
                for i in 0..count * 3 {
                    let c = m.u8(base + i);
                    if c > 0 {
                        m.set8(base + i, (c + amount) & 0xff);
                    }
                }
            }
            28 => {
                for i in 0..count * 3 {
                    let c = m.u8(base + i);
                    if c - amount > 0 {
                        m.set8(base + i, c - amount);
                    }
                }
            }
            29 => {
                let mut pal = m.slice(base, count * 3);
                pal.rotate_right(3);
                m.write(base, &pal);
            }
            _ => {
                let mut pal = m.slice(base, count * 3);
                pal.rotate_left(3);
                m.write(base, &pal);
            }
        }
    }

    /// 0x162d: scene palette = slot 0, empty (white) entries filled from
    /// slots 3, 1, 2; then 8 -> 6 bit.
    pub fn merge_palette(&mut self) {
        let m = &mut self.mem;
        let mut pal = m.slice(g::PAL_SLOT[0], 768);
        for slot in [3, 1, 2] {
            let src = m.slice(g::PAL_SLOT[slot], 768);
            for i in 0..256 {
                let empty = |p: &[u8]| p[i * 3..i * 3 + 3] == [0xff, 0xff, 0xff];
                if empty(&pal) && !empty(&src) {
                    pal[i * 3..i * 3 + 3].copy_from_slice(&src[i * 3..i * 3 + 3]);
                }
            }
        }
        for v in pal.iter_mut() {
            *v /= 4;
        }
        m.write(g::PAL_SCENE, &pal);
    }

    pub fn set_dac(&mut self, pal6: &[u8]) {
        self.dac.copy_from_slice(&pal6[..768]);
    }

    /// 0xccab: fade the DAC to black, one level per step.
    pub fn fade_out(&mut self) {
        for _ in 0..64 {
            let mut any = false;
            for v in self.dac.iter_mut() {
                if *v != 0 {
                    *v -= 1;
                    any = true;
                }
            }
            if !any {
                break;
            }
            self.wait(8.0);
        }
    }

    /// 0xcd25: fade the DAC up to `pal6`.
    pub fn fade_in(&mut self, pal6: &[u8]) {
        for _ in 0..64 {
            let mut any = false;
            for (v, &t) in self.dac.iter_mut().zip(pal6) {
                let t = t & 63;
                if *v < t {
                    *v += 1;
                    any = true;
                } else if *v > t {
                    *v -= 1;
                    any = true;
                }
            }
            if !any {
                break;
            }
            self.wait(8.0);
        }
    }

    // ------------------------------------------------------------ files and pictures

    /// Bytes of FILE entry n (by archive offset, or by name with loose files).
    pub fn file_bytes(&self, n: i32) -> Result<Vec<u8>, String> {
        if !(0..20).contains(&n) {
            return Err(String::new());
        }
        let (name, off, size) = data::file_entry(&self.mem, n as usize);
        self.src.resource(&name, off, size).ok_or_else(|| String::from_utf8_lossy(&name).into_owned())
    }

    /// Load graphics slot `slot` from its name/offset entry; returns false if missing.
    pub fn load_sheet(&mut self, slot: usize, fatal_missing: u32) {
        let (name, off, size) = data::gfx_entry(&self.mem, slot);
        let label = String::from_utf8_lossy(&name).into_owned();
        let Some(bytes) = self.src.resource(&name, off, size) else { self.fatal(fatal_missing, &label) };
        match data::decode_pcx(&bytes) {
            Ok((pix, pal)) => {
                self.sheets[slot] = pix;
                self.mem.write(g::PAL_SLOT[slot], &pal);
            }
            Err(code) => self.fatal(code, &label),
        }
    }

    /// LOADPCX(file, slot).
    fn load_pcx(&mut self, file: i32, slot: i32) {
        if (0..4).contains(&slot) {
            let bytes = self.file_bytes(file).unwrap_or_else(|n| self.fatal(0x6e, &n));
            let s = slot as usize;
            match data::decode_pcx(&bytes) {
                Ok((pix, pal)) => {
                    self.sheets[s] = pix;
                    self.mem.write(g::PAL_SLOT[s], &pal);
                }
                Err(code) => self.fatal(code, ""),
            }
            self.merge_palette();
            self.mem.set8(g::PAL_DIRTY, 1);
        }
        if (0..20).contains(&file) && (0..4).contains(&slot) {
            let (f, s) = (file as usize, slot as usize);
            let name = self.mem.slice(ds::FILE_NAMES + f * 13, 13);
            self.mem.write(ds::GFX_NAMES + s * 13, &name);
            let tab = self.mem.slice(ds::FILE_TAB + f * 8, 8);
            self.mem.write(ds::GFX_TAB + s * 8, &tab);
        }
    }

    /// PLAYCMF(file).
    fn play_cmf(&mut self, file: i32) {
        let bytes = self.file_bytes(file).unwrap_or_else(|n| self.fatal(0x68, &n));
        let _ = self.mixer.lock().unwrap().play_music(&bytes);
        self.mem.set8(g::MUSIC_ON, 1);
        self.mem.set16(g::MUSIC, file);
    }

    /// SHOWPCX(file, mode, secs): a blocking full-screen picture.
    fn show_pcx(&mut self, file: i32, mode: i32, secs: i32) {
        self.mem.set8(g::PAUSED, 1);
        let saved = self.mem.i16(g::CURSOR_STATE);
        self.mem.set16(g::CURSOR_STATE, 2);
        self.fade_out();
        let bytes = self.file_bytes(file).unwrap_or_else(|n| self.fatal(0x68, &n));
        let (pix, pal) = data::decode_pcx(&bytes).unwrap_or_else(|c| self.fatal(c, ""));
        self.vga.copy_from_slice(&pix);
        let pal6: Vec<u8> = pal.iter().map(|v| v / 4).collect();
        self.mem.write(g::PAL_PICTURE, &pal6);
        self.fade_in(&pal6);
        self.mem.set8(g::KEY, 0);
        let ticks = secs * self.mem.u8(0x97);
        match mode {
            0 => self.wait_input(None),
            1 => self.wait_countdown(ticks, false),
            2 => self.wait_countdown(ticks, true),
            3 => {
                self.wait_countdown(ticks, false);
                self.wait_input(None);
            }
            _ => {}
        }
        self.fade_out();
        self.full_redraw();
        self.present_all();
        let pal = self.mem.slice(g::PAL_SCENE, 768);
        self.fade_in(&pal);
        self.mem.set8(g::KEY, 0);
        self.mem.set8(g::PAUSED, 0);
        self.mem.set16(g::CURSOR_STATE, saved);
    }

    fn wait_countdown(&mut self, ticks: i32, interruptible: bool) {
        self.mem.set16(g::COUNTDOWN, ticks);
        self.buttons[0].presses = 0;
        self.buttons[1].presses = 0;
        while self.mem.i16(g::COUNTDOWN) > 0 {
            self.wait(RETRACE_MS);
            if interruptible && self.any_input() {
                break;
            }
        }
    }

    /// Wait for a click (0xced7) or a key.
    pub fn wait_input(&mut self, _limit: Option<f64>) {
        loop {
            self.wait(RETRACE_MS);
            if self.any_input() {
                return;
            }
        }
    }

    pub fn any_input(&mut self) -> bool {
        self.button(0) || self.button(1) || self.mem.u8(g::KEY) != 0
    }

    /// int 33h fn 5 + status: pressed since the last query and still held.
    pub fn button(&mut self, b: usize) -> bool {
        let pressed = self.buttons[b].presses > 0;
        self.buttons[b].presses = 0;
        pressed && self.buttons[b].held
    }
}

impl Core for Engine {
    fn host(&mut self) -> &mut Host<Self> {
        &mut self.host
    }
    fn tick_ms(&self) -> f64 {
        TICK_MS
    }
    fn isr_tick(&mut self) {
        self.timer_tick()
    }
    fn input_event(&mut self, ev: Input) {
        match ev {
            Input::MouseMove(x, y) => self.pointer = (x, y),
            Input::Button(b, down) => {
                if let Some(s) = self.buttons.get_mut(b) {
                    if down && !s.held {
                        s.presses += 1;
                    }
                    s.held = down;
                }
            }
            Input::KeyDown(scan, key) => {
                if key.is_some() {
                    self.last_char = key;
                }
                if scan > 0 && scan < 128 {
                    self.mem.set8(g::KEY, scan as i32);
                    self.keys[scan as usize] = 1;
                }
            }
            Input::KeyUp(scan) => {
                if scan > 0 && scan < 128 {
                    self.keys[scan as usize] = 0;
                }
            }
        }
    }
    fn frame(&mut self, error: Option<String>) -> Frame {
        self.present();
        Frame { screen: self.screen.clone(), palette: self.palette.to_vec(), exited: self.exited, error }
    }
    fn run(&mut self) {
        self.main_loop()
    }
    fn set_exited(&mut self) {
        self.exited = true;
    }
}

pub type EngineHandle = host::EngineHandle<Engine>;

impl Engine {
    pub fn spawn(self) -> EngineHandle {
        let mixer = self.mixer.clone();
        host::spawn(self, mixer)
    }
}
