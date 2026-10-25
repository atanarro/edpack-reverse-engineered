//! The EDPACK v2.0 interpreter: a re-implementation of RUNDAT.EXE /
//! RUNFILES.EXE.
//!
//! The original is a classic DOS program: a main loop (0x5687) that waits two
//! VGA retraces per pass (≈28.5 ms), a 50 Hz timer interrupt decrementing
//! countdowns, and blocking routines (fades, FLI playback, menus) that run
//! their own loops.  Here the engine keeps exactly that structure and runs on
//! its own thread: whenever it needs time to pass ([`Engine::wait`]) it hands
//! the current frame to the host and blocks until the host advances the clock
//! and delivers input.  Host and engine never run at the same time, so play is
//! deterministic for a given input sequence.  See [`EngineHandle`].

use std::collections::HashMap;
use std::panic;
use std::sync::{Arc, Mutex};

use crate::Error;
use crate::audio::Mixer;
use crate::audio::voc::{Voice, parse_voc};
use crate::dat::{ByteReader, Special, file_type};
use crate::fli::FliDecoder;
use crate::font::Font;
use crate::game::Game;
use crate::host::{self, Core, Fatal, Host};
pub use crate::host::{Frame, Input, Key};
use crate::level::{file_entry, gfx_entry, load_data_file, load_level, thing_name};
use crate::memory::{Memory, THING_SIZE, ds, v102};
use crate::pcx::decode_pcx;
use crate::player as p;
use crate::specials::{INV_RECORD, InventoryFile, MouseFile, load_iac, load_title};
use crate::ui::{self, bar, inv, mouse as mo, stext as st, text as tx};
use crate::vm::{self, MAX_OPS, MAX_OPS_V102, VAR_REF, op};

/// One main-loop pass: two retraces of a 70.086 Hz VGA.
pub const PASS_MS: f64 = 2000.0 / 70.086;
/// The engine timer runs at 50 Hz.
pub const TICK_MS: f64 = 20.0;

pub mod pal {
    pub const ORIG: usize = 0x2280; // base palette (MODPAL reads it, PUTPAL writes it) 6-bit
    pub const CUR: usize = 0x2580; // current palette (uploaded to the DAC) 6-bit
    pub const ESC: usize = 0x2b80; // ESCENARIOPCX palette, 8-bit
    pub const DIB: usize = 0x2e80; // DIBUJOSPCX
    pub const INV: usize = 0x3180; // inventory (NIVEL.INV)
    pub const MOVS: usize = 0x3480; // MOVSPCX
}

pub mod g {
    pub const LOADED_LEVEL: usize = 0xfc9d;
    pub const PAL_DIRTY: usize = 0xfcd3;
    pub const ANIM_WAIT: usize = 0xfcd0;
    pub const ANIM_TICKS: usize = 0x8a23;
    pub const PENDING_TEXT: usize = 0xfd36;
    pub const TARGET: usize = 0xfcdf;
    pub const PAUSED: usize = 0x98;
    pub const KEY: usize = 0xdc;
    pub const WAIT: usize = 0xfcdc;
    pub const DRAWANIM_DIRTY: usize = 0xfd24;
    pub const IAC: usize = 0x698e;
    pub const MUSIC_ON: usize = 0xfca6;
    pub const THINGS_ON: usize = 0xfcff;
    pub const MENU_ON: usize = 0xfd04;
    pub const VERBS_ON: usize = 0xfd03;
    pub const INV_ON: usize = 0xfcfe;
    pub const CMP_FLAG: usize = 0xfccb;
    pub const DUMMY: usize = 0xfff0;
}

fn message(code: u32) -> &'static str {
    match code {
        0x0b => "Archivo '%s' no es del tipo correcto.",
        0x32 => "ERROR : - No se pueden añadir mas objetos al inventario -",
        0x34 => {
            "Se han ejecutado mas de 50 commando seguidos en una misma ANIMATION o ACTION sin devolver el control con un RET();, probablemente haya un bucle sin salida."
        }
        0x63 => "No encuentro archivo de graficos '%s'.",
        0x65 => "Error al abrir el nivel '%s'.",
        0x66 => "Error al abrir el archivo de graficos(1) '%s'.",
        0x67 => "Error al abrir el  archivo de graficos(0) '%s'.",
        0x68 | 0x6e => "Error al abrir el archivo '%s'.",
        0x69 => "Nivel de comienzo no especificado en el Nivel 0",
        0x6a => "Fichero de los MOVIMIENTOS no especificado en el nivel 0",
        0x6c => "No encuentro archivo de graficos(2) '%s'.",
        0x6f => "Commando no valida.",
        0x72 => "Demasiados usos de DRAWANIM en este nivel.",
        0x73 => "No hay textos definidos para STEXT",
        0x74 => "El NEWMOUSE() '%s' no existe en el fichero .MOUSE",
        0x75 => "El NEWMOVS() '%s' no existe en el fichero .MOVS",
        0x76 => "El Objeto del Inventario '%s' no existe en el fichero .OBJECT",
        _ => "Error interno %s",
    }
}

/// SETUP.CFG speed (byte 1) -> (ticks per walk step, ticks between animation runs).
const SETUP_SPEED: [(i32, i32); 5] = [(15, 10), (8, 5), (4, 2), (3, 1), (1, 0)];
/// SETUP.CFG text speed (byte 2) -> ticks per character.
const SETUP_TEXT: [i32; 5] = [16, 12, 8, 4, 2];

/// Where savegames go.
pub trait Storage: Send {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn set(&mut self, key: &str, data: &[u8]);
}

/// In-memory storage (tests).
#[derive(Default)]
pub struct MemStorage(pub HashMap<String, Vec<u8>>);

impl Storage for MemStorage {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
    fn set(&mut self, key: &str, data: &[u8]) {
        self.0.insert(key.to_string(), data.to_vec());
    }
}

/// Saves as files in a directory.
pub struct DirStorage(pub std::path::PathBuf);

impl Storage for DirStorage {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(key)).ok()
    }
    fn set(&mut self, key: &str, data: &[u8]) {
        let path = self.0.join(key);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, data) {
            eprintln!("No se pudo guardar la partida: {e}");
        }
    }
}

pub struct Options {
    /// SETUP.CFG game speed 0..4 (the demo shipped with 2).
    pub speed: usize,
    /// SETUP.CFG text speed 0..4 (the demo shipped with 3).
    pub text_speed: usize,
    pub seed: u64,
}

impl Default for Options {
    fn default() -> Self {
        Options { speed: 2, text_speed: 3, seed: 0x1234_5678_9abc_def1 }
    }
}

#[derive(Default, Clone, Copy)]
struct ButtonState {
    held: bool,
    presses: u32,
}

pub struct Engine {
    pub mem: Memory,
    pub code: Vec<i16>,
    pub game: Game,
    pub font: Font,
    inv_file: InventoryFile,
    mou_file: MouseFile,
    pub title: Vec<u8>,
    pub bg: Vec<u8>,
    pub dib: Vec<u8>,
    pub movs: Vec<u8>,
    pub vga: Vec<u8>,
    pub screen: Vec<u8>,
    pub inv_records: Vec<u8>,
    pub dac: [u8; 768],
    pub palette: [u8; 768],
    pub exited: bool,
    host: Host<Engine>,
    buttons: [ButtonState; 2],
    keys: [u8; 128],
    pointer: (i32, i32),
    pub last_char: Option<Key>,
    music_name: Option<Vec<u8>>,
    /// The CMF buffer: the last tune loaded by PLAYCMF.
    cmf: Vec<u8>,
    voice: Option<Voice>,
    pub mixer: Arc<Mutex<Mixer>>,
    storage: Box<dyn Storage>,
    rng: u64,
    /// RUNDAT v1.02 behaviour (Dadton): see docs/v2-rundat/07-versions.md.
    pub v102: bool,
    /// RUNDAT v1.03: 16-bit program counters (the action's at CUR_ACTION_PC,
    /// with its compare flag moved out of the way).
    pub wide_pc: bool,
}

/// The current action's program counter: u8, or u16 with RUNDAT v1.03.
fn action_pc(m: &Memory, wide: bool) -> i32 {
    if wide { m.u16(ds::CUR_ACTION_PC) } else { m.u8(ds::CUR_ACTION_PC) }
}

fn set_action_pc(m: &mut Memory, wide: bool, v: i32) {
    if wide { m.set16(ds::CUR_ACTION_PC, v) } else { m.set8(ds::CUR_ACTION_PC, v) }
}

impl Engine {
    /// Where the current action's compare flag lives (it follows the u8 pc;
    /// RUNDAT v1.03 moved it to make room for a u16 pc).
    fn action_flag(&self) -> usize {
        if self.wide_pc { v102::ACTION_FLAG_V103 } else { g::CMP_FLAG }
    }

    pub fn new(game: Game, storage: Box<dyn Storage>, opts: Options) -> Result<Engine, Error> {
        let fnt =
            game.special(Special::Fnt).ok_or_else(|| Error::new("No encuentro el fichero de LETRAS 'NIVEL.FNT'."))?;
        let font = Font::new(&fnt.0[fnt.1..])?;
        let inv_file = InventoryFile::load(game.special(Special::Inv))?;
        let mou_file = MouseFile::load(game.special(Special::Mou))?;
        let title = load_title(game.special(Special::Tit))?;
        let iac = load_iac(game.special(Special::Iac))?;
        let v102 = game.level_format().is_v102();
        let wide_pc = game.level_format().wide_pc();
        let mut e = Engine {
            mem: if v102 { Memory::with_vars(Memory::v102_vars()) } else { Memory::new() },
            code: vec![0; 5000],
            game,
            font,
            inv_file,
            mou_file,
            title,
            bg: vec![0; 64000],
            dib: vec![0; 64000],
            movs: vec![0; 64000],
            vga: vec![0; 64000],
            screen: vec![0; 64000],
            inv_records: vec![0; 30 * INV_RECORD],
            dac: [0; 768],
            palette: [0; 768],
            exited: false,
            host: Host::default(),
            buttons: [ButtonState::default(); 2],
            keys: [0; 128],
            pointer: (160, 100),
            last_char: None,
            music_name: None,
            cmf: Vec::new(),
            voice: None,
            mixer: Arc::new(Mutex::new(Mixer::new(44100.0))),
            storage,
            rng: opts.seed | 1,
            v102,
            wide_pc,
        };
        e.mem.write(g::IAC, &iac);
        e.init_globals(opts.speed, opts.text_speed);
        Ok(e)
    }

    /// Variable defaults set by main() (0xb78c).
    fn init_globals(&mut self, speed: usize, text_speed: usize) {
        let m = &mut self.mem;
        let (step, anim) = SETUP_SPEED.get(speed).copied().unwrap_or(SETUP_SPEED[2]);
        m.set8(p::STEP_TICKS, step);
        m.set8(g::ANIM_TICKS, anim);
        m.set8(tx::TICKS_PER_CHAR, SETUP_TEXT.get(text_speed).copied().unwrap_or(4));
        m.set16(0xdd, 50);
        m.set8(0xfd65, 2);
        m.set8(0xfd64, 3);
        for a in 0xcf..=0xdb {
            m.set8(a, 0xff);
        }
        m.set16(p::SPEED_X, 10);
        m.set16(p::SPEED_Y, 4);
        for (i, v) in [0xfa, 0xfb, 0xfc, 0xfd, 0xfe].into_iter().enumerate() {
            m.set16(0xfbe1 + 2 * i, v);
        }
        m.set16(0xfbeb, 0xfe);
        m.set16(0xfbed, 0xfd);
        m.set16(0xfbdf, 0xfa);
        m.set16(tx::COLOR, 1);
        m.set16(tx::BORDER, 15);
        m.set16(tx::PBORDER, 15);
        m.set8(ds::LEVEL, 0);
        m.set8(g::LOADED_LEVEL, 0xff);
        m.set16(p::PATH, -100);
        m.set16(p::PATH + 2, -100);
        m.set16(bar::COLOR, 2);
        m.set16(bar::BORDER, 15);
        m.set16(bar::Y, 189);
        m.set16(0xfce8, 0);
        m.set16(0xfcea, 0);
        m.set16(0xfd05, 5);
        m.set16(0xfce0, 2);
        m.set16(0xfce2, 15);
        m.set16(0xfce4, 6);
        m.set16(0xfce6, 15);
        m.set16(st::ACTIVE, -1);
        for a in [g::THINGS_ON, g::INV_ON, g::VERBS_ON, bar::ENABLED, g::MENU_ON] {
            m.set8(a, 1);
        }
        m.set16(0xfd07, 10);
        m.set16(0xfd09, 0);
        m.set16(p::AREA_H, 187);
        m.set16(0xfd30, 2);
        m.set16(0xfd32, 0);
        m.set16(0xfd34, 4);
        let def = b"\xad\xad\xad ATENCION !!!^Este texto no esta definido.\0";
        for i in 0..5 {
            m.write(ds::MESSAGES + i * 250, def);
        }
        m.set16(mo::STATE, 0);
        m.set16(mo::X, 100);
        m.set16(mo::Y, 100);
        m.set16(mo::W, 10);
        m.set16(mo::H, 10);
        m.set16(0x4338, -1);
        if self.v102 {
            for (a, v) in [
                (v102::VAR28_MENU, 1),
                (v102::VAR29_BAR, 1),
                (v102::VAR30_HOVER, 1),
                (v102::VAR31_INV, 1),
                (v102::VAR32_VERBS, 1),
                (v102::VAR47_SKIP_KEY, 1),
                (v102::VAR50_VERBS, 5),
                (v102::VAR51_LOOK, 1),
                (v102::VAR52_USE, 3),
                (v102::VAR53_MENU_CURSOR, 5),
                (v102::VAR54_ARROW_CURSOR, 5),
                (v102::SKIP_TARGET, -1),
            ] {
                m.set16(a, v);
            }
            for i in 0..50 {
                m.set16(v102::ANIM_SKIP + 2 * i, -1);
            }
            m.set8(v102::INV_VERB, 0xff);
            // hw_init seeds the counter with the DOS time's hundredths
            m.set8(v102::RAND_COUNTER, (self.rng % 100) as i32);
        }
    }

    /// MOUSEACTION argument for the menu: 101 (-> cursor 5), or var53 in v1.02.
    pub fn menu_cursor(&self) -> i32 {
        if self.v102 { self.mem.i16(v102::VAR53_MENU_CURSOR) } else { 101 }
    }

    /// The UI switches (vars 28..32): bytes in v1.0, independent words in v1.02.
    pub fn flag_on(&self, demo_addr: usize) -> bool {
        if !self.v102 {
            return self.mem.u8(demo_addr) == 1;
        }
        let a = match demo_addr {
            g::MENU_ON => v102::VAR28_MENU,
            bar::ENABLED => v102::VAR29_BAR,
            g::THINGS_ON => v102::VAR30_HOVER,
            g::INV_ON => v102::VAR31_INV,
            g::VERBS_ON => v102::VAR32_VERBS,
            a => a,
        };
        self.mem.i16(a) == 1
    }

    // ------------------------------------------------------------ threading

    /// Start the engine on its own thread.
    pub fn spawn(self) -> EngineHandle {
        let mixer = self.mixer.clone();
        host::spawn(self, mixer)
    }

    /// Let `ms` milliseconds of emulated time pass, running the 50 Hz timer
    /// interrupt meanwhile (the original's waits and retrace syncs).
    pub fn wait(&mut self, ms: f64) {
        host::wait(self, ms)
    }

    pub fn clock(&self) -> f64 {
        self.host.clock
    }

    fn on_input(&mut self, ev: Input) {
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
                    self.keys[scan as usize] = 1;
                    self.mem.set8(g::KEY, scan as i32);
                }
            }
            Input::KeyUp(scan) => {
                if scan > 0 && scan < 128 {
                    self.keys[scan as usize] = 0;
                }
            }
        }
    }

    // ------------------------------------------------------------ ISR, input

    fn timer_tick(&mut self) {
        if self.v102 {
            let c = self.mem.u8(v102::RAND_COUNTER) + 1;
            self.mem.set8(v102::RAND_COUNTER, if c > 100 { 0 } else { c });
        }
        self.sample_mouse();
        let m = &mut self.mem;
        // The ISR's cursor state machine: 2/4 = erase request -> 3 (hidden),
        // 5 = redraw request (e.g. from MMOUSE) -> 0 (shown).
        match m.i16(mo::STATE) {
            2 | 4 => m.set16(mo::STATE, 3),
            5 => m.set16(mo::STATE, 0),
            _ => {}
        }
        if m.u8(g::PAUSED) == 0 {
            if m.i16(tx::ACTIVE) == 1 && m.i16(tx::TIMER) > 0 {
                let v = m.i16(tx::TIMER) - 1;
                m.set16(tx::TIMER, v);
            }
            for a in [p::STEP_WAIT, g::ANIM_WAIT] {
                if m.u8(a) > 0 {
                    let v = m.u8(a) - 1;
                    m.set8(a, v);
                }
            }
        }
        if m.i16(g::WAIT) > 0 {
            let v = m.i16(g::WAIT) - 1;
            m.set16(g::WAIT, v);
        }
    }

    fn sample_mouse(&mut self) {
        let m = &mut self.mem;
        if m.u8(0x97) == 1 {
            return;
        }
        let (w, h) = (m.i16(mo::W), m.i16(mo::H));
        let x = (self.pointer.0 - m.i16(mo::HOTX)).min(321 - w).max(0);
        let y = (self.pointer.1 - m.i16(mo::HOTY)).min(200 - h).max(0);
        m.set16(mo::X, x);
        m.set16(mo::Y, y);
    }

    pub fn hotspot(&self) -> (i32, i32) {
        let m = &self.mem;
        (m.i16(mo::X) + m.i16(mo::HOTX), m.i16(mo::Y) + m.i16(mo::HOTY))
    }

    pub fn set_mouse(&mut self, x: i32, y: i32) {
        self.mem.set16(mo::X, x);
        self.mem.set16(mo::Y, y);
        self.pointer = (x + self.mem.i16(mo::HOTX), y + self.mem.i16(mo::HOTY));
    }

    /// INT 33h fn 5 + status test: pressed since last query and still the only button held.
    pub fn button(&mut self, b: usize) -> bool {
        let pressed = self.buttons[b].presses > 0;
        self.buttons[b].presses = 0;
        pressed && self.buttons[b].held && !self.buttons[1 - b].held
    }

    pub fn key(&self) -> i32 {
        self.mem.u8(g::KEY)
    }

    pub fn hide_cursor(&mut self) {
        self.mem.set16(mo::STATE, 3);
    }
    pub fn show_cursor(&mut self) {
        self.mem.set16(mo::STATE, 0);
    }

    /// Compose vga + mouse cursor into `screen`, DAC -> 8-bit palette.
    pub fn present(&mut self) {
        self.screen.copy_from_slice(&self.vga);
        let m = &self.mem;
        if m.i16(mo::STATE) != 3 && m.i16(mo::MODE) != 2 {
            let (cx, cy, sx, sy) = (m.i16(mo::X), m.i16(mo::Y), m.i16(mo::SRCX), m.i16(mo::SRCY));
            for y in 0..m.i16(mo::H) {
                for x in 0..m.i16(mo::W) {
                    let (dx, dy, px, py) = (cx + x, cy + y, sx + x, sy + y);
                    if !(0..320).contains(&dx)
                        || !(0..200).contains(&dy)
                        || !(0..320).contains(&px)
                        || !(0..200).contains(&py)
                    {
                        continue;
                    }
                    let c = self.movs[(py * 320 + px) as usize];
                    if c != 0 {
                        self.screen[(dy * 320 + dx) as usize] = c;
                    }
                }
            }
        }
        for i in 0..768 {
            let v = self.dac[i] & 63;
            self.palette[i] = (v << 2) | (v >> 4);
        }
    }

    pub fn fatal(&self, code: u32, arg: &str) -> ! {
        panic::panic_any(Fatal(message(code).replace("%s", arg)))
    }

    fn fail(&self, e: Error) -> ! {
        panic::panic_any(Fatal(e.0))
    }

    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    // ------------------------------------------------------------ main loop

    fn main_loop(&mut self) {
        while !self.exited {
            self.pass();
        }
    }

    fn pass(&mut self) {
        if self.mem.u8(g::KEY) == 0x39 && self.flag_on(g::MENU_ON) {
            self.mem.set8(g::PAUSED, 1);
            crate::menu::run_menu(self);
            self.mem.set8(g::KEY, 0);
            self.mem.set8(g::PAUSED, 0);
            self.mem.set8(st::HL, 0xff);
            if self.exited {
                return;
            }
        }
        if self.mem.u8(inv::OPEN) == 1 {
            ui::inventory_frame(self);
            if self.mem.u8(inv::OPEN) == 0 {
                ui::build_label(self);
                self.show_cursor();
            }
            self.compose();
            self.wait(PASS_MS);
            return;
        }
        if self.mem.i16(st::ACTIVE) != -1 {
            ui::stext_frame(self);
            self.compose();
            self.wait(PASS_MS);
            return;
        }
        if self.mem.u8(ds::LEVEL) != self.mem.u8(g::LOADED_LEVEL) {
            self.change_level();
            let l = self.mem.u8(ds::LEVEL);
            self.mem.set8(g::LOADED_LEVEL, l);
            if self.exited {
                return;
            }
        }
        self.run_animations();
        self.run_action();
        if self.exited {
            return;
        }
        if self.mem.i16(tx::ACTIVE) == 1 {
            ui::text_frame(self);
        }
        self.input();
        p::tick(&mut self.mem);
        if self.mem.u8(bar::STATE) == 2 {
            ui::build_label(self);
        }
        p::layer_swap(&mut self.mem);
        let a = self.mem.u8(ds::CUR_ACTION);
        if (1..=49).contains(&a) || self.mem.i16(tx::ACTIVE) == 1 {
            self.mem.set8(bar::STATE, 0);
            self.mem.set8(bar::LAST, 0xff);
            self.mem.set8(bar::LABEL_THING, 0);
        }
        self.zones();
        self.compose();
        if self.mem.u8(g::PAL_DIRTY) == 1 {
            self.upload_palette();
            self.mem.set8(g::PAL_DIRTY, 0);
        }
        self.wait(PASS_MS);
    }

    fn upload_palette(&mut self) {
        let cur = self.mem.slice(pal::CUR, 768);
        self.dac.copy_from_slice(&cur);
    }

    /// SCALEMOV / SHADOWMOV re-evaluated when the player moved.
    fn zones(&mut self) {
        let m = &self.mem;
        if m.i16(p::X) != m.i16(0xfc05) || m.i16(p::Y) != m.i16(0xfc07) {
            p::apply_scale_mov(&mut self.mem);
            self.apply_shadow();
            let (x, y) = (self.mem.i16(p::X), self.mem.i16(p::Y));
            self.mem.set16(0xfc05, x);
            self.mem.set16(0xfc07, y);
        }
    }

    fn apply_shadow(&mut self) {
        let Some(shade) = p::shadow_mov(&mut self.mem) else { return };
        let (first, count) = (self.mem.i16(p::SHADOW_FIRST), self.mem.i16(p::SHADOW_COUNT));
        if (0..256).contains(&first) && count > 0 && first + count < 256 {
            self.mod_pal(first, count, shade);
        }
        self.mem.set8(g::PAL_DIRTY, 1);
    }

    // ------------------------------------------------------------ scripts

    fn word(&self, i: i32) -> i32 {
        let i = (i & 0xffff) as usize;
        self.code.get(i).copied().unwrap_or(0) as i32
    }

    fn value(&self, w: i32) -> i32 {
        if w >= VAR_REF { self.mem.get_var(w - VAR_REF) } else { w }
    }

    /// The bytecode interpreter (0x2ca2).  `base` is the script's word offset;
    /// the u8 program counter and compare flag live in memory at `pc_addr` /
    /// `flag_addr`.  Returns 0 when the script ended, otherwise non-zero
    /// (1 = yielded; DELAY returns its counter's low byte, like the original).
    /// Blocking commands (SHOWPCX, PLAYFLI, MENU, EXIT) run to completion inline.
    /// `anim` is the animation index when an animation runs, -1 otherwise
    /// (only RUNDAT v1.02 uses it).
    pub fn run_script(&mut self, base: i32, action_addr: usize, pc_addr: usize, flag_addr: usize, anim: i32) -> i32 {
        let mut count = 0;
        let limit = if self.v102 { MAX_OPS_V102 } else { MAX_OPS };
        loop {
            count += 1;
            if count > limit {
                self.fatal(0x34, "");
            }
            let wide = self.wide_pc;
            let pc = if wide { self.mem.u16(pc_addr) } else { self.mem.u8(pc_addr) };
            let at = base + pc;
            let opc = self.word(at);
            if opc == op::END {
                return vm::FINISHED;
            }
            let (raw0, raw1) = (self.word(at + 1), self.word(at + 2));
            let (a0, a1) = (self.value(raw0), self.value(raw1));
            let (a2, a3) = (self.value(self.word(at + 3)), self.value(self.word(at + 4)));
            let next = |e: &mut Self, n: i32| {
                if wide { e.mem.set16(pc_addr, pc + n) } else { e.mem.set8(pc_addr, pc + n) }
            };
            let flag = self.mem.u8(flag_addr);
            macro_rules! jump {
                ($cond:expr) => {{
                    if $cond {
                        // v1.03 stores the whole target, older builds its low byte
                        if wide { self.mem.set16(pc_addr, raw0) } else { self.mem.set8(pc_addr, raw0) }
                    } else {
                        next(self, 2)
                    }
                    continue;
                }};
            }
            match opc {
                op::GOLEVEL => {
                    self.mem.set8(ds::LEVEL, a0);
                    self.mem.set8(ds::NEXT_DOOR, a1);
                    self.mem.set8(ds::NEXT_MODE, a2);
                    next(self, 4);
                    return vm::YIELD;
                }
                op::GOXY => {
                    p::go_xy(&mut self.mem, a0, a1);
                    next(self, 3);
                    return vm::YIELD;
                }
                op::SPEAK => {
                    if self.mem.i16(tx::ACTIVE) != 0 {
                        return vm::YIELD;
                    }
                    ui::speak(self, a0);
                    next(self, 2);
                    return vm::YIELD;
                }
                op::PTEXT => {
                    if self.mem.i16(tx::ACTIVE) != 0 {
                        return vm::YIELD;
                    }
                    ui::ptext(self, a0, a1, a2, a3);
                    next(self, 5);
                    return vm::YIELD;
                }
                op::ACTION => {
                    // writes the global current-action slot, then keeps running
                    // with the same base (quirk of the original)
                    self.mem.set8(ds::CUR_ACTION, a0);
                    set_action_pc(&mut self.mem, self.wide_pc, 0);
                }
                op::VAR => {
                    self.mem.set_var(raw0, raw1);
                    next(self, 3);
                }
                op::VARVAR => {
                    let v = self.mem.get_var(raw1);
                    self.mem.set_var(raw0, v);
                    next(self, 3);
                }
                op::ADD | op::ADDVAR | op::SUB | op::SUBVAR => {
                    let d = if opc == op::ADD || opc == op::SUB { raw1 } else { self.mem.get_var(raw1) };
                    let v = self.mem.get_var(raw0);
                    self.mem.set_var(raw0, if opc <= op::ADDVAR { v + d } else { v - d });
                    next(self, 3);
                }
                op::ANIM_ON | op::ANIM_OFF => {
                    self.mem.set16(ds::ANIMS + (a0 as usize).wrapping_mul(10), (opc == op::ANIM_ON) as i32);
                    next(self, 2);
                }
                op::RET => {
                    next(self, 1);
                    return vm::YIELD;
                }
                op::JMP => jump!(true),
                op::CMP => {
                    let f = if a0 == a1 {
                        1
                    } else if a0 < a1 {
                        2
                    } else {
                        3
                    };
                    self.mem.set8(flag_addr, f);
                    next(self, 3);
                }
                op::JE => jump!(flag == 1),
                op::JNE => jump!(flag != 1),
                op::JL => jump!(flag == 2),
                op::JA => jump!(flag == 3),
                op::JLE => jump!(flag == 2 || flag == 1),
                op::JAE => jump!(flag == 3 || flag == 1),
                op::ADDINV => {
                    self.add_inv(a0);
                    next(self, 2);
                }
                op::SUBINV => {
                    self.sub_inv(a0);
                    next(self, 2);
                }
                op::SHADOW => {
                    self.mem.set16(p::SHADOW_FIRST, a0);
                    self.mem.set16(p::SHADOW_COUNT, a1);
                    next(self, 3);
                    self.mem.set8(g::PAL_DIRTY, 1);
                }
                op::MODPAL => {
                    if (0..256).contains(&a0) && a1 > 0 && a0 + a1 < 256 {
                        self.mod_pal(a0, a1, a2);
                    }
                    next(self, 4);
                    self.mem.set8(g::PAL_DIRTY, 1);
                }
                op::RPAL | op::LPAL => {
                    self.rotate_pal(a0, a1, if opc == op::RPAL { 1 } else { -1 });
                    next(self, 3);
                    self.mem.set8(g::PAL_DIRTY, 1);
                }
                op::NEWTHING => {
                    self.new_thing(a0, a1);
                    next(self, 3);
                }
                op::SHOWPCX => {
                    next(self, 4);
                    self.show_pcx(a0, a1, a2);
                }
                op::LOADPCX => {
                    self.load_pcx(a0, a1);
                    self.mem.set8(g::PAL_DIRTY, 1);
                    next(self, 3);
                }
                op::DRAW => {
                    self.compose();
                    self.upload_palette();
                    next(self, 1);
                }
                op::SPEEDXY | op::SCALEXY | op::DEFSCR => {
                    let (ax, ay) = match opc {
                        op::SPEEDXY => (p::SPEED_X, p::SPEED_Y),
                        op::SCALEXY => (p::RED_W, p::RED_H),
                        _ => (0xfd07, 0xfd09),
                    };
                    if a0 >= 0 {
                        self.mem.set16(ax, a0);
                    }
                    if a1 >= 0 {
                        self.mem.set16(ay, a1);
                    }
                    next(self, 3);
                }
                op::PLAYCMF => {
                    self.play_cmf(a0);
                    next(self, 2);
                }
                op::STOPCMF => {
                    self.stop_cmf();
                    next(self, 1);
                }
                op::PLAYVOC => {
                    if let Some(v) = self.voice.clone() {
                        self.mixer.lock().unwrap().play_voice(v);
                    }
                    next(self, 1);
                }
                op::PPOS => {
                    p::face(&mut self.mem, a0);
                    next(self, 2);
                }
                op::GETVOC => {
                    let bytes = self.file_bytes(a0).unwrap_or_else(|| self.fatal(0x68, &self.file_name(a0)));
                    self.voice = parse_voc(&bytes).ok();
                    next(self, 2);
                }
                op::EXIT => {
                    next(self, 1);
                    self.fade_out();
                    self.stop_cmf();
                    self.exited = true;
                    return vm::FINISHED;
                }
                op::STEXT => {
                    ui::stext_start(self, a0, a1, a2);
                    next(self, 4);
                    return vm::YIELD;
                }
                op::MOUSEACTION => {
                    self.mouse_action(a0);
                    next(self, 2);
                }
                op::PMOV => {
                    let v = self.mem.i16(p::VISIBLE) + 1;
                    self.mem.set16(p::PREV_VISIBLE, v);
                    if a0 < 0 {
                        self.mem.set8(p::PMOV, 0);
                    } else {
                        let c = ds::COORANIM + (a0 as usize) * 10;
                        self.mem.set8(p::PMOV, 1);
                        for k in 0..4 {
                            let v = self.mem.i16(c + k * 2);
                            self.mem.set16(p::PMOV_X + k * 2, v);
                        }
                    }
                    next(self, 2);
                }
                op::MMOUSE => {
                    self.mem.set16(mo::MODE, a0);
                    self.mem.set16(mo::STATE, 5);
                    next(self, 2);
                }
                op::MENU => {
                    next(self, 1);
                    self.mem.set8(g::PAUSED, 1);
                    crate::menu::run_menu(self);
                    self.mem.set8(g::KEY, 0);
                    self.mem.set8(g::PAUSED, 0);
                }
                op::RAND => {
                    let r = if self.v102 {
                        // trunc(f32(c * f32(n / 100))), c cycling 0..100 at 50 Hz
                        let c = self.mem.u8(v102::RAND_COUNTER) as f32;
                        (c * (a1 as f32 / 100.0)) as i32
                    } else {
                        let n = a1 % 100;
                        (self.random() * n as f64).floor() as i32
                    };
                    self.mem.set_var(a0, r);
                    next(self, 3);
                }
                op::ASLT if self.v102 => {
                    self.mem.set8(g::KEY, 0);
                    if anim != -1 {
                        self.mem.set16(v102::ANIM_SKIP + 2 * anim.clamp(0, 49) as usize, raw0);
                    } else {
                        self.mem.set16(v102::SKIP_TARGET, raw0);
                    }
                    next(self, 2);
                }
                op::DSLT if self.v102 => {
                    // QUIRK: word +0x0c of record `anim` = `active` of record anim+1
                    let i = (anim as i8 as i32 + 1) as usize;
                    self.mem.set16(ds::ANIMS + i.wrapping_mul(10), -1);
                    next(self, 1);
                }
                op::PUTXY if self.v102 => {
                    if a0 != -100 {
                        let w = self.mem.i16(p::W);
                        self.mem.set16(p::X, a0 - w / 2);
                    }
                    if a1 != -100 {
                        let h = self.mem.i16(p::H);
                        self.mem.set16(p::Y, a1 - h);
                    }
                    next(self, 3);
                }
                op::SEEANIM if self.v102 => {
                    let on = self.mem.i16(ds::ANIMS + (a0 as usize).wrapping_mul(10)) == 1;
                    self.mem.set16(v102::VAR48_SEEANIM, on as i32);
                    next(self, 2);
                }
                op::SEEINV => {
                    // the original's search loop never executes (bug): var 22 is always 0
                    self.mem.set16(0xfd20, 0);
                    next(self, 2);
                }
                op::NEWMOVS => {
                    self.new_movs(a0);
                    next(self, 2);
                }
                op::DRAWANIM => {
                    self.draw_anim(a0, a1, a2);
                    next(self, 4);
                }
                op::LOADRGB => {
                    self.load_rgb(a0, a1);
                    self.mem.set8(g::PAL_DIRTY, 1);
                    next(self, 3);
                }
                op::PUTPAL => {
                    let cur = self.mem.slice(pal::CUR, 768);
                    self.mem.write(pal::ORIG, &cur);
                    next(self, 1);
                }
                op::NEWMOUSE => {
                    self.new_mouse(a0);
                    next(self, 2);
                }
                op::PLAYFLI => {
                    next(self, 4);
                    self.play_fli(a0, a1, a2);
                }
                op::FIRSTFRAME => {
                    self.first_frame(a0, a1);
                    next(self, 3);
                }
                op::NEXTFRAME => {
                    let f = ds::DEFFRAMES + (a0 as usize) * 52;
                    let cur = self.mem.u8(f + 0x33) + 1;
                    self.mem.set8(f + 0x33, cur);
                    let item = self.mem.u8(f + 1 + (cur & 0xff) as usize);
                    if item == 0xff {
                        self.first_frame(a0, a1)
                    } else {
                        self.new_thing(a1, item)
                    }
                    next(self, 3);
                }
                op::DELAY => {
                    // the counter lives in the script's second argument word
                    let ci = ((at + 2) & 0xffff) as usize;
                    let c = (self.word(at + 2) + 1) as i16 as i32;
                    if ci < self.code.len() {
                        self.code[ci] = c as i16;
                    }
                    if c < a0 {
                        return c & 0xff; // 0 here makes the caller think the script ended (quirk)
                    }
                    if ci < self.code.len() {
                        self.code[ci] = 0;
                    }
                    next(self, 3);
                }
                op::LOADDATA => {
                    self.load_data(a0, a1);
                    next(self, 3);
                }
                _ => {
                    self.mem.set8(action_addr, 0);
                    self.fatal(0x6f, "");
                }
            }
            if self.exited {
                return vm::FINISHED;
            }
        }
    }

    fn first_frame(&mut self, frame: i32, thing: i32) {
        let f = ds::DEFFRAMES + (frame as usize) * 52;
        self.mem.set8(f + 0x33, 0);
        let item = self.mem.u8(f + 1);
        self.new_thing(thing, item);
    }

    fn run_animations(&mut self) {
        if self.mem.u8(g::ANIM_WAIT) != 0 {
            return;
        }
        for i in 0..50 {
            let a = ds::ANIMS + i * 10;
            if self.mem.i16(a) == 1 {
                let base = self.mem.i16(a + 2);
                let r = self.run_script(base, g::DUMMY, a + 4, a + 6, i as i32);
                if r == vm::FINISHED {
                    self.mem.set16(a + 4, 0);
                    self.mem.set16(a, 0);
                }
                if self.exited {
                    return;
                }
            }
            if self.v102 {
                // ASLT: checked for every record, active or not
                let skip_at = v102::ANIM_SKIP + 2 * i;
                let skip = self.mem.i16(skip_at);
                if skip != -1 {
                    if self.mem.u8(g::KEY) == self.mem.i16(v102::VAR47_SKIP_KEY) {
                        self.mem.set16(a + 4, skip);
                        self.mem.set8(g::KEY, 0);
                    }
                    if self.mem.i16(a + 4) >= skip {
                        self.mem.set16(skip_at, -1);
                        ui::clear_texts(self);
                    }
                }
            }
        }
        let t = self.mem.u8(g::ANIM_TICKS);
        self.mem.set8(g::ANIM_WAIT, t);
    }

    fn thing_addr(t: i32) -> usize {
        ds::THINGS + (t as usize) * THING_SIZE
    }

    /// Name of inventory object `obj` as stored in NIVEL.INV.
    pub fn object_name(&self, obj: i32) -> Vec<u8> {
        if obj < 0 || obj as usize >= self.inv_file.count {
            return Vec::new();
        }
        let rec = self.inv_record(obj);
        let name = &rec[0xfa..0xfa + 30];
        name[..name.iter().position(|&c| c == 0).unwrap_or(30)].to_vec()
    }

    /// NIVEL.INV record of `obj` as this build's ADDINV reads it.
    fn inv_record(&self, obj: i32) -> Vec<u8> {
        if self.v102 { self.inv_file.record_v102(obj as usize) } else { self.inv_file.record(obj as usize) }
    }

    pub fn thing_name(&self, t: i32) -> Vec<u8> {
        thing_name(&self.mem, t as usize)
    }

    fn walk_to(&mut self, t: i32) {
        let a = Self::thing_addr(t);
        let (x, y) = (self.mem.i16(a + 0x40), self.mem.i16(a + 0x42));
        p::go_xy(&mut self.mem, x, y);
    }

    fn face_thing(&mut self, t: i32) {
        let d = self.mem.i16(Self::thing_addr(t) + 0x44);
        if (0..=3).contains(&d) {
            p::face(&mut self.mem, d);
        }
    }

    /// run_action (0x7937): verb approach states and the current ACTION script.
    fn run_action(&mut self) {
        let m = &self.mem;
        let (cur, pc) = (m.u8(ds::CUR_ACTION), action_pc(m, self.wide_pc));
        let (t, v) = (m.u8(inv::HELD_OBJ), m.u8(mo::CLICK_VERB));
        let walking = m.u8(p::WALKING) != 0;
        let set = |e: &mut Self, a: i32, pc: i32| {
            e.mem.set8(ds::CUR_ACTION, a);
            set_action_pc(&mut e.mem, e.wide_pc, pc);
        };
        let actions = |e: &Self, verb: i32| e.mem.i16(Self::thing_addr(t) + 0x48 + 2 * verb as usize);
        match cur {
            100 => match pc {
                0 => {
                    self.walk_to(t);
                    set(self, 100, 1);
                }
                1 if !walking => {
                    self.face_thing(t);
                    set(self, 100, 2);
                }
                2 => {
                    let id = self.mem.i16(g::PENDING_TEXT);
                    if id != 0 {
                        ui::speak(self, id);
                    }
                    set(self, 0, 2);
                }
                _ => {}
            },
            101 => match pc {
                0 => {
                    self.walk_to(t);
                    set(self, 101, 1);
                }
                1 if !walking => {
                    self.face_thing(t);
                    set(self, 101, 2);
                }
                2 => {
                    let a = actions(self, v);
                    if a != 0 {
                        set(self, a & 0xff, 0);
                    }
                }
                _ => {}
            },
            102 | 104 => {
                let tg = self.mem.u8(g::TARGET);
                match pc {
                    0 => {
                        self.walk_to(tg);
                        set(self, cur, 1);
                    }
                    1 if !walking => {
                        self.face_thing(tg);
                        set(self, cur, 3);
                    }
                    3 => {
                        self.mem.set8(inv::HELD, 0);
                        let res = self.mem.u8(inv::RESULT);
                        if cur == 102 && res != 0 {
                            set(self, res, 0);
                        } else {
                            ui::speak(self, if cur == 104 && res != 0 { res } else { 2002 });
                            self.mem.set8(ds::CUR_ACTION, 0);
                        }
                    }
                    _ => {}
                }
            }
            106 => match pc {
                0 if !walking => {
                    self.face_thing(t);
                    set(self, 106, 1);
                }
                1 => {
                    let a = actions(self, v);
                    if a != 4000 {
                        set(self, (a - 4000) & 0xff, 0);
                    }
                }
                _ => {}
            },
            _ => {
                if cur == 0 {
                    return;
                }
                if self.v102 && self.mem.i16(v102::SKIP_TARGET) != -1 {
                    let target = self.mem.i16(v102::SKIP_TARGET);
                    if self.mem.u8(g::KEY) == self.mem.i16(v102::VAR47_SKIP_KEY) {
                        set_action_pc(&mut self.mem, self.wide_pc, target);
                        self.mem.set8(g::KEY, 0);
                    }
                    if action_pc(&self.mem, self.wide_pc) >= target {
                        // reached or skipped past the label: end texts, stop walking
                        self.mem.set16(v102::SKIP_TARGET, -1);
                        ui::clear_texts(self);
                        self.mem.set16(p::PATH, -100);
                        self.mem.set16(p::PATH + 2, -100);
                        self.mem.set8(p::WALKING, 0);
                    }
                }
                let walking = self.mem.u8(p::WALKING) != 0;
                if self.mem.i16(tx::ACTIVE) == 0 && !walking {
                    let base = self.mem.i16(ds::ACTION_OFFS + 2 * cur as usize);
                    if self.run_script(base, ds::CUR_ACTION, ds::CUR_ACTION_PC, self.action_flag(), -1) == vm::FINISHED
                    {
                        set(self, 0, 0);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ input

    fn input(&mut self) {
        let m = &self.mem;
        let mode = m.i16(mo::MODE);
        let state = m.i16(mo::STATE);
        if state == 2 || state == 3 {
            return;
        }
        if m.i16(tx::ACTIVE) != 0 || state != 0 {
            if mode >= 2 {
                return;
            }
            let (l, r) = (self.button(0), self.button(1));
            if l || r {
                ui::clear_texts(self);
            }
            return;
        }
        let cur = m.u8(ds::CUR_ACTION);
        if cur != 0 && cur < 50 {
            return;
        }
        let buttons_ok = mode < 2 || mode == 4;
        if m.u8(inv::OPEN) == 0
            && m.i16(mo::Y) <= 0
            && self.flag_on(g::INV_ON)
            && buttons_ok
            && self.mem.u8(inv::PENDING) == 0
        {
            let verb = self.mem.u8(mo::VERB);
            self.mem.set8(inv::OPEN, 1);
            self.mem.set8(mo::SAVED_VERB, verb);
            if self.v102 {
                // the panel opens with the verb last used inside it
                let (iv, look, use_) =
                    (self.mem.u8(v102::INV_VERB), self.mem.i16(v102::VAR51_LOOK), self.mem.i16(v102::VAR52_USE));
                if iv != look && iv != use_ {
                    self.mem.set8(v102::INV_VERB, look);
                }
                let iv = self.mem.u8(v102::INV_VERB);
                self.mouse_action(iv);
            } else if verb != 1 && verb != 3 {
                self.mouse_action(1);
            }
            return;
        }
        let h = self.mem.u8(mo::HOVER);
        self.mem.set8(mo::PREV_HOVER, h);
        self.mem.set8(mo::HOVER, 0);
        if self.flag_on(g::THINGS_ON) {
            ui::hover(self);
        }
        if self.mem.u8(mo::PREV_HOVER) != self.mem.u8(mo::HOVER) {
            self.mem.set8(bar::STATE, 2);
        }
        if buttons_ok && self.button(1) && self.flag_on(g::VERBS_ON) {
            self.mouse_action(100);
        }
        if self.mem.u8(inv::HELD) == 0xff && self.mem.u8(inv::PENDING) == 0xff {
            self.mem.set8(inv::HELD, 0);
            self.mem.set8(inv::PENDING, 0);
            self.mem.set8(bar::STATE, 2);
        }
        if self.mem.u8(inv::HELD) == 1 && self.mem.u8(inv::PENDING) == 1 {
            if self.mem.u8(inv::SECOND) == 100 {
                self.acinvth();
                self.mem.set8(inv::PENDING, 0);
            } else {
                self.acinvinv();
                self.mem.set8(inv::HELD, 0xff);
                self.mem.set8(inv::PENDING, 0xff);
            }
        }
        if !self.button(0) || !buttons_ok {
            return;
        }
        let verb = self.mem.u8(mo::VERB);
        self.mem.set8(mo::CLICK_VERB, verb);
        let (hx, hy) = self.hotspot();
        self.mem.set16(mo::CLICKX, hx);
        self.mem.set16(mo::CLICKY, hy);
        let text_on = self.mem.i16(tx::ACTIVE) != 0;
        if !text_on && self.mem.u8(ds::CUR_ACTION) != 0 && state != 2 {
            self.mem.set8(ds::CUR_ACTION, 0);
        }
        p::stop(&mut self.mem);
        let (held, hov) = (self.mem.u8(inv::HELD), self.mem.u8(mo::HOVER));
        let drop = held != 0 && !text_on && self.mem.u16(inv::HOVER) == 0 && hov == 0;
        if drop && !self.v102 {
            self.mem.set8(inv::HELD, 0);
        }
        if self.mem.u8(inv::HELD) != 1
            && self.mem.u8(ds::CUR_ACTION) == 0
            && hov == 0
            && hy <= self.mem.i16(p::AREA_H)
            && verb == 0
        {
            p::go_xy(&mut self.mem, hx, hy);
        }
        if drop && self.v102 {
            // v1.02: drop without walking, back to the verb used before the inventory
            let v = self.mem.u8(mo::SAVED_VERB);
            self.mouse_action(v);
            self.mem.set8(inv::HELD, 0);
        }
        if self.mem.u8(inv::HELD) != 0 && !text_on && hov != 0 {
            self.mem.set8(inv::SECOND, 100);
            self.mem.set8(inv::SECOND_OBJ, hov - 1);
            self.mem.set8(inv::PENDING, 1);
            self.mem.set8(bar::STATE, 2);
        }
        if hov != 0 && self.mem.u8(inv::HELD) != 1 && !text_on && verb < 5 {
            self.verb_click();
        }
    }

    /// verb_click (0x8362): decode THING.ACTIONS[verb].
    fn verb_click(&mut self) {
        let t = self.mem.u8(mo::HOVER) - 1;
        let v = self.mem.u8(mo::VERB);
        self.mem.set8(inv::HELD_OBJ, t);
        self.mem.set8(mo::CLICK_VERB, v);
        let a = self.mem.i16(Self::thing_addr(t) + 0x48 + 2 * v as usize);
        let mut set = |s: i32, pc: i32, text: Option<i32>| {
            if let Some(id) = text {
                self.mem.set16(g::PENDING_TEXT, id);
            }
            self.mem.set8(ds::CUR_ACTION, s);
            set_action_pc(&mut self.mem, self.wide_pc, pc);
        };
        if a >= 5000 {
            set(100, 1, Some(a - 5000));
        } else if a >= 4000 {
            set(106, 0, None);
        } else if a >= 3000 {
            set(100, 2, Some(a - 3000));
        } else if a >= 2000 {
            set(100, 0, Some(a - 2000));
        } else if a >= 1000 {
            set((a - 1000) & 0xff, 0, None);
        } else if a > 0 {
            set(101, 0, None);
        } else if a == 0 && v == 0 {
            set(100, 0, Some(0));
        }
    }

    /// Inventory object used on a THING (0x7e0b).
    fn acinvth(&mut self) {
        let (o, t) = (self.mem.u8(inv::HELD_OBJ), self.mem.u8(inv::SECOND_OBJ));
        let mut found = false;
        for i in 0..30 {
            let a = ds::ACINVTH + i * 8;
            let (mode, iv, th) = (self.mem.i16(a), self.mem.i16(a + 2), self.mem.i16(a + 4));
            if mode == 0 {
                continue;
            }
            if self.v102 {
                // v1.02: the cursor goes back to the verb used before the inventory
                let v = self.mem.u8(mo::SAVED_VERB);
                self.mouse_action(v);
            }
            if (iv == o || iv == -100) && (th == t || th == -100) {
                let res = self.mem.i16(a + 6);
                self.mem.set8(inv::RESULT, res);
                set_action_pc(&mut self.mem, self.wide_pc, 0);
                let state = match mode {
                    1 => Some((102, 0)),
                    2 => Some((102, 3)),
                    3 => Some((102, 1)),
                    4 => Some((104, 0)),
                    5 => Some((104, 3)),
                    6 => Some((104, 1)),
                    _ => None,
                };
                if let Some((s, pc)) = state {
                    self.mem.set8(ds::CUR_ACTION, s);
                    set_action_pc(&mut self.mem, self.wide_pc, pc);
                }
                found = true;
                break;
            }
        }
        if !found {
            if self.v102 {
                let v = self.mem.u8(mo::SAVED_VERB);
                self.mouse_action(v);
            }
            self.mem.set8(inv::RESULT, 0);
            self.mem.set8(ds::CUR_ACTION, 102);
            set_action_pc(&mut self.mem, self.wide_pc, 0);
        }
        self.mem.set8(g::TARGET, t);
        self.mem.set8(inv::PENDING, 0);
    }

    /// Combine two inventory objects through NIVEL.IAC (0x7d29).
    fn acinvinv(&mut self) {
        let (x, y) = (self.mem.u8(inv::HELD_OBJ), self.mem.u8(inv::SECOND_OBJ));
        for i in 0..50 {
            let a = g::IAC + i * 10;
            if self.mem.i16(a) != 1 {
                continue;
            }
            if self.v102 {
                let v = self.mem.u8(mo::SAVED_VERB);
                self.mouse_action(v);
            }
            let (oa, ob, res, mode) =
                (self.mem.i16(a + 2), self.mem.i16(a + 4), self.mem.i16(a + 6), self.mem.i16(a + 8));
            if (x == oa && y == ob) || (x == ob && y == oa) {
                if mode == 0 || mode == 1 {
                    self.sub_inv(oa);
                }
                if mode == 0 || mode == 2 {
                    self.sub_inv(ob);
                }
                self.add_inv(res);
                ui::speak(self, 2004);
                return;
            }
        }
        ui::speak(self, 2001);
    }

    pub fn mouse_action(&mut self, n: i32) {
        let v102 = self.v102;
        let m = &mut self.mem;
        let mut v = if n == 100 { m.u8(mo::VERB) + 1 } else { n };
        if v102 {
            // cycles through var50 verbs, no other clamping
            if n == 100 && v >= m.i16(v102::VAR50_VERBS) {
                v = 0;
            }
        } else {
            if v > 4 && v != 101 {
                v = 0;
            }
            if v == 101 {
                v = 5;
            }
        }
        m.set8(mo::VERB, v);
        let v = m.u8(mo::VERB);
        // v1.02 has 10 cursors; entries 6..9 live outside the demo's table
        let r = if v102 && (6..10).contains(&v) {
            v102::CURSORS_6_9 + (v - 6) as usize * 12
        } else {
            mo::TABLE + v as usize * 12
        };
        let (old_hx, old_hy) = (m.i16(mo::HOTX), m.i16(mo::HOTY));
        let nx = m.i16(mo::X) + old_hx - m.i16(r + 8);
        let ny = m.i16(mo::Y) + old_hy - m.i16(r + 10);
        for (dst, k) in [(mo::SRCX, 0), (mo::SRCY, 2), (mo::W, 4), (mo::H, 6), (mo::HOTX, 8), (mo::HOTY, 10)] {
            let val = m.i16(r + k);
            m.set16(dst, val);
        }
        m.set16(mo::X, nx);
        m.set16(mo::Y, ny);
        m.set8(inv::HELD, 0);
        ui::build_label(self);
    }

    // ------------------------------------------------------------ files

    fn file_name(&self, n: i32) -> String {
        if !(0..20).contains(&n) {
            return String::new();
        }
        crate::cp850_to_string(&file_entry(&self.mem, n as usize).name)
    }

    /// Find a file by name in the level's FILE and gfx tables (0x1798).
    fn lookup_file(&self, name: &[u8]) -> Option<Vec<u8>> {
        let mut entry = None;
        for i in 0..20 {
            let f = file_entry(&self.mem, i);
            if f.name == name {
                entry = Some(f);
                break;
            }
        }
        for i in 0..4 {
            let f = gfx_entry(&self.mem, i);
            if f.name == name {
                entry = Some(f);
                break;
            }
        }
        self.game.resource(&entry?)
    }

    fn file_bytes(&self, n: i32) -> Option<Vec<u8>> {
        if !(0..20).contains(&n) {
            return None;
        }
        let f = file_entry(&self.mem, n as usize);
        if f.name.is_empty() {
            return None;
        }
        self.game.resource(&f).or_else(|| self.lookup_file(&f.name))
    }

    /// Load a 320x200 PCX into the buffer `which`, palette (8-bit) into DS `pal_addr`.
    fn load_picture(&mut self, name: &[u8], which: Buffer, pal_addr: usize, err: u32) {
        let bytes = self.lookup_file(name).unwrap_or_else(|| self.fatal(err, &crate::cp850_to_string(name)));
        let img = decode_pcx(&bytes).unwrap_or_else(|e| self.fail(e));
        if img.width != 320 || img.height != 200 {
            self.fail(Error::new(format!(
                "El archivo de graficos '{}' no es del formato 320*200 256 colores.",
                crate::cp850_to_string(name)
            )));
        }
        self.buffer(which).copy_from_slice(&img.pixels);
        self.mem.write(pal_addr, &img.palette);
    }

    fn buffer(&mut self, b: Buffer) -> &mut Vec<u8> {
        match b {
            Buffer::Bg => &mut self.bg,
            Buffer::Dib => &mut self.dib,
            Buffer::Movs => &mut self.movs,
        }
    }

    // ------------------------------------------------------------ palettes

    fn merge_palettes(&mut self) {
        let m = &mut self.mem;
        let white = |m: &Memory, a: usize, c: usize| (0..3).all(|k| m.u8(a + c * 3 + k) == 255);
        let esc = m.slice(pal::ESC, 768);
        m.write(pal::CUR, &esc);
        for c in 0..256 {
            if !white(m, pal::CUR, c) {
                continue;
            }
            for src in [pal::INV, pal::DIB, pal::MOVS] {
                if !white(m, src, c) {
                    for k in 0..3 {
                        let v = m.u8(src + c * 3 + k);
                        m.set8(pal::CUR + c * 3 + k, v);
                    }
                }
            }
        }
        for i in 0..768 {
            let v = m.u8(pal::CUR + i) >> 2;
            m.set8(pal::CUR + i, v);
            m.set8(pal::ORIG + i, v);
        }
    }

    pub fn mod_pal(&mut self, first: i32, count: i32, amount: i32) {
        let n = if count > 255 { 256 - first } else { count };
        for k in (3 * first)..(3 * (first + n)) {
            let v = self.mem.u8(pal::ORIG + k as usize) + amount;
            self.mem.set8(pal::CUR + k as usize, if amount >= 0 { v.min(63) } else { v.max(0) });
        }
    }

    fn rotate_pal(&mut self, first: i32, mut count: i32, dir: i32) {
        if first + count > 255 {
            count = 256 - first;
        }
        if count <= 0 || first < 0 {
            return;
        }
        let base = pal::CUR + first as usize * 3;
        let n = count as usize * 3;
        let colours = &mut self.mem.bytes[base..base + n];
        if dir > 0 { colours.rotate_right(3) } else { colours.rotate_left(3) }
    }

    pub fn set_dac(&mut self, pal6: &[u8]) {
        self.dac.copy_from_slice(&pal6[..768]);
    }

    fn fade_out(&mut self) {
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

    fn fade_to(&mut self, pal6: &[u8]) {
        for _ in 0..64 {
            let mut any = false;
            for (v, &t) in self.dac.iter_mut().zip(pal6) {
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

    // ------------------------------------------------------------ drawing

    fn draw_thing(&mut self, t: usize) {
        let m = &self.mem;
        let a = ds::THINGS + t * THING_SIZE;
        blit(
            &mut self.vga,
            &self.dib,
            m.i16(a + 9),
            m.i16(a + 0x0b),
            m.i16(a + 0x19),
            m.i16(a + 0x1b),
            m.i16(a + 0x21),
            m.i16(a + 0x23),
            m.u8(a + 0x2d) == 1,
            m.i16(p::AREA_H),
        );
    }

    /// Rebuild the frame: background, things, player, texts, info bar, panels.
    pub fn compose(&mut self) {
        self.vga.copy_from_slice(&self.bg);
        let things_on = self.flag_on(g::THINGS_ON);
        let layer = |e: &mut Self, front: bool| {
            for k in 0..30 {
                let t = e.mem.i8(ds::THING_ORDER + k);
                if !(0..30).contains(&t) {
                    continue;
                }
                let a = ds::THINGS + t as usize * THING_SIZE;
                let (mode, l) = (e.mem.u8(a + 0x2b), e.mem.u8(a + 0x15));
                if mode == 0 || mode == 3 {
                    continue;
                }
                if if front { l == 1 || l == 3 } else { l == 0 || l == 2 } {
                    e.draw_thing(t as usize);
                }
            }
        };
        if things_on {
            layer(self, false);
        }
        if self.mem.i16(p::VISIBLE) != 0 {
            let src = if self.mem.u8(p::PMOV) != 0 { &self.dib } else { &self.movs };
            p::draw(&self.mem, &mut self.vga, src);
        }
        if things_on {
            layer(self, true);
        }
        ui::draw_texts(self);
        if self.mem.u8(inv::OPEN) == 1 {
            ui::draw_inventory(self);
        }
        if self.mem.i16(st::ACTIVE) != -1 {
            ui::draw_stext(self);
        }
        ui::draw_info_bar(self);
    }

    // ------------------------------------------------------------ levels

    fn change_level(&mut self) {
        loop {
            self.hide_cursor();
            let wide = self.wide_pc;
            let m = &mut self.mem;
            m.set8(ds::CUR_ACTION, 0);
            set_action_pc(m, wide, 0);
            m.set8(inv::OPEN, 0);
            m.set8(mo::VERB, 0);
            m.set8(mo::SAVED_VERB, 0);
            let movs_name = m.slice(ds::GFX_NAMES + 26, 13);
            let movs_tab = m.slice(ds::GFX_TAB + 16, 8);
            let n = m.u8(ds::LEVEL) as usize;
            let (packed, format) = (self.game.is_packed(), self.game.level_format());
            let Some((bytes, off)) = self.game.level(n) else { self.fatal(0x65, &n.to_string()) };
            match load_level(&mut self.mem, bytes, off, packed, format) {
                Ok(l) => {
                    self.code = l.code;
                    if self.v102 {
                        // the skip targets: word +0x0a of each 12-byte record
                        for (i, r) in l.anims.chunks_exact(12).enumerate() {
                            self.mem.set16(v102::ANIM_SKIP + 2 * i, i16::from_le_bytes([r[10], r[11]]) as i32);
                        }
                    }
                }
                Err(e) => self.fail(e),
            }
            if n == 0 {
                self.level0_setup();
                if self.exited {
                    return;
                }
                if self.mem.u8(ds::LEVEL) != 0 {
                    continue;
                }
                self.fatal(0x69, "");
            }
            let dib = self.mem.cstr(ds::GFX_NAMES + 13, 13);
            self.load_picture(&dib, Buffer::Dib, pal::DIB, 0x66);
            let esc = self.mem.cstr(ds::GFX_NAMES, 13);
            self.load_picture(&esc, Buffer::Bg, pal::ESC, 0x67);
            self.mem.write(ds::GFX_NAMES + 26, &movs_name);
            self.mem.write(ds::GFX_TAB + 16, &movs_tab);
            self.merge_palettes();
            self.place_at_door();
            p::apply_scale_mov(&mut self.mem);
            self.apply_shadow();
            let saved = self.mem.u8(ds::CUR_ACTION);
            self.run_action0();
            if self.exited {
                return;
            }
            p::update_source(&mut self.mem);
            let (x, y) = (self.mem.i16(p::X), self.mem.i16(p::Y));
            self.mem.set16(0xfc05, x);
            self.mem.set16(0xfc07, y);
            self.mem.set8(ds::CUR_ACTION, saved);
            set_action_pc(&mut self.mem, self.wide_pc, 0);
            let mode = self.mem.u8(ds::NEXT_MODE);
            self.transition(mode);
            self.show_cursor();
            self.mem.set8(g::KEY, 0);
            return;
        }
    }

    fn place_at_door(&mut self) {
        let wide = self.wide_pc;
        let m = &mut self.mem;
        let d = ds::DOORS + m.u8(ds::NEXT_DOOR) as usize * 10;
        let (w, h) = (m.i16(p::MOV_W), m.i16(p::MOV_H));
        m.set16(p::W, w);
        m.set16(p::H, h);
        if m.i16(d) != 0 {
            m.set8(p::PMOV, 0);
            if m.i16(d + 2) != -100 {
                let v = m.i16(d + 2) - p::idiv2(w);
                m.set16(p::X, v);
            }
            if m.i16(d + 4) != -100 {
                let v = m.i16(d + 4) - h;
                m.set16(p::Y, v);
            }
            let dir = m.i16(d + 6);
            if dir != -100 {
                let f = m.u8(p::FRAME);
                m.set8(p::PREV_FRAME, f);
                p::face(m, dir);
            }
            let f = m.u8(p::FRAME) as usize;
            let (sx, sy) = (m.i16(p::POINTS + f * 4), m.i16(p::POINTS + f * 4 + 2));
            m.set16(p::SRCX, sx);
            m.set16(p::SRCY, sy);
            let act = m.i16(d + 8);
            m.set8(ds::CUR_ACTION, act);
        } else {
            let side = m.u8(p::S_SIDE);
            m.set8(p::FRAME, side);
            let f = side as usize;
            let v = m.i16(p::POINTS + f * 4 + 2);
            m.set16(p::SRCX, v); // original bug: fc09 written twice, fc0b untouched
            m.set8(ds::CUR_ACTION, 0);
        }
        set_action_pc(m, wide, 0);
    }

    /// ACTION 0 of a level runs to completion at once; its yields are ignored.
    fn run_action0(&mut self) {
        self.mem.set8(ds::CUR_ACTION, 0);
        set_action_pc(&mut self.mem, self.wide_pc, 0);
        let base = self.mem.i16(ds::ACTION_OFFS);
        if base == 0 {
            return;
        }
        for _ in 0..100_000 {
            let r = self.run_script(base, ds::CUR_ACTION, ds::CUR_ACTION_PC, self.action_flag(), -1);
            if r == vm::FINISHED || self.exited {
                return;
            }
            if self.mem.u8(p::WALKING) != 0 {
                self.mem.set8(p::WALKING, 0);
            }
            if self.mem.i16(tx::ACTIVE) != 0 {
                self.mem.set16(tx::ACTIVE, 0);
            }
        }
    }

    fn level0_setup(&mut self) {
        self.run_action0();
        if self.exited {
            return;
        }
        let name = self.mem.cstr(ds::GFX_NAMES + 26, 13);
        if name.is_empty() {
            self.fatal(0x6a, "");
        }
        self.load_picture(&name, Buffer::Movs, pal::MOVS, 0x6c);
        let pal_inv = self.inv_file.palette.clone();
        self.mem.write(pal::INV, &pal_inv);
    }

    fn transition(&mut self, mode: i32) {
        let cur = self.mem.slice(pal::CUR, 768);
        let area_h = self.mem.i16(p::AREA_H);
        match mode {
            0 => {
                self.fade_out();
                self.compose();
                self.fade_to(&cur);
            }
            1..=4 => {
                let old = self.vga.clone();
                self.compose();
                let next = self.vga.clone();
                self.set_dac(&cur);
                let mut step = self.mem.i16(0xfd07);
                if step == 0 {
                    step = 10;
                    self.mem.set16(0xfd07, 10);
                }
                let ov = self.mem.i16(0xfd09);
                let rows = (area_h + 1).min(200);
                let px = |buf: &[u8], x: i32, y: i32| {
                    if (0..320).contains(&x) && (0..200).contains(&y) { buf[(y * 320 + x) as usize] } else { 0 }
                };
                let mut s = step;
                loop {
                    if if mode <= 2 { s >= 319 - ov } else { s >= area_h + 1 - ov } {
                        break;
                    }
                    for y in 0..rows.max(0) {
                        for x in 0..320 {
                            let v = match mode {
                                1 => {
                                    if x < 320 - s {
                                        px(&old, x + s, y)
                                    } else {
                                        px(&next, x - (320 - s) + ov, y)
                                    }
                                }
                                2 => {
                                    if x < s {
                                        px(&next, x + 320 - s - ov, y)
                                    } else {
                                        px(&old, x - s, y)
                                    }
                                }
                                3 => {
                                    if y < s {
                                        px(&next, x, area_h - s - ov + y)
                                    } else {
                                        px(&old, x, y - s)
                                    }
                                }
                                _ => {
                                    if y < area_h + 1 - s {
                                        px(&old, x, y + s)
                                    } else {
                                        px(&next, x, y - (area_h + 1 - s) + ov)
                                    }
                                }
                            };
                            self.vga[(y * 320 + x) as usize] = v;
                        }
                    }
                    self.wait(12.0);
                    s += step;
                }
                self.compose();
            }
            5 | 6 => {
                if mode == 5 {
                    self.vga.fill(0);
                }
                self.set_dac(&cur);
                self.compose();
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ engine services

    pub fn new_thing(&mut self, t: i32, c: i32) {
        if !(0..30).contains(&t) || !(0..100).contains(&c) {
            return;
        }
        let (a, ca) = (Self::thing_addr(t), ds::COORANIM + c as usize * 10);
        let v: Vec<i32> = (0..4).map(|k| self.mem.i16(ca + k * 2)).collect();
        self.mem.set16(a + 9, v[0]);
        self.mem.set16(a + 0x0b, v[1]);
        self.mem.set16(a + 0x21, v[2]);
        self.mem.set16(a + 0x23, v[3]);
    }

    fn draw_anim(&mut self, c: i32, x: i32, y: i32) {
        let ca = ds::COORANIM + (c.clamp(0, 99) as usize) * 10;
        let m = &self.mem;
        blit(&mut self.bg, &self.dib, m.i16(ca), m.i16(ca + 2), x, y, m.i16(ca + 4), m.i16(ca + 6), false, 200);
        let dib_name = m.cstr(ds::GFX_NAMES + 13, 13);
        for i in 0..25 {
            let a = ds::DRAWANIM_TAB + i * 20;
            if m.u8(a + 0xd) != 0
                && m.i16(a + 0xe) == c
                && m.i16(a + 0x10) == x
                && m.i16(a + 0x12) == y
                && m.cstr(a, 13) == dib_name
            {
                self.mem.set8(a + 0xd, 2);
                self.mem.set8(g::DRAWANIM_DIRTY, 1);
                return;
            }
        }
        for i in 0..25 {
            let a = ds::DRAWANIM_TAB + i * 20;
            if self.mem.u8(a + 0xd) == 0 {
                let mut name = [0u8; 13];
                name[..dib_name.len().min(13)].copy_from_slice(&dib_name[..dib_name.len().min(13)]);
                self.mem.write(a, &name);
                self.mem.set8(a + 0xd, 2);
                self.mem.set16(a + 0xe, c);
                self.mem.set16(a + 0x10, x);
                self.mem.set16(a + 0x12, y);
                self.mem.set8(g::DRAWANIM_DIRTY, 1);
                return;
            }
        }
        self.fatal(0x72, "");
    }

    pub fn add_inv(&mut self, obj: i32) {
        for s in 0..30 {
            let a = inv::SLOTS + s * 6;
            if self.mem.u8(a) != 0 {
                continue;
            }
            if obj < 0 || obj as usize >= self.inv_file.count {
                self.fatal(0x76, &obj.to_string());
            }
            self.mem.set16(a + 2, obj);
            self.mem.set8(a, 1);
            self.mem.set8(a + 1, 0);
            let rec = self.inv_record(obj);
            self.inv_records[s * INV_RECORD..(s + 1) * INV_RECORD].copy_from_slice(&rec);
            return;
        }
        self.fatal(0x32, "");
    }

    pub fn sub_inv(&mut self, obj: i32) {
        for s in 0..30 {
            let a = inv::SLOTS + s * 6;
            if self.mem.i16(a + 2) != obj {
                continue;
            }
            self.mem.set8(a, 0);
            self.mem.set16(a + 2, 0);
            for i in s..30 {
                let b = inv::SLOTS + i * 6;
                if i == 29 {
                    self.mem.set8(b, 0);
                    self.mem.set16(b + 2, 0);
                    self.mem.set16(b + 4, -1);
                    break;
                }
                let (used, o) = (self.mem.u8(b + 6), self.mem.i16(b + 8));
                self.mem.set8(b, used);
                self.mem.set16(b + 2, o);
                self.mem.set16(b + 4, -1);
                if used == 0 {
                    break;
                }
                self.inv_records.copy_within((i + 1) * INV_RECORD..(i + 2) * INV_RECORD, i * INV_RECORD);
            }
            while self.mem.u8(inv::FIRST) > 0
                && self.mem.u8(inv::SLOTS + (self.mem.u8(inv::FIRST) as usize + 7) * 6) == 0
            {
                let f = self.mem.u8(inv::FIRST) - 1;
                self.mem.set8(inv::FIRST, f);
            }
            return;
        }
    }

    fn new_movs(&mut self, n: i32) {
        let Some((b, off)) = self.game.special(Special::Mov) else { self.fatal(0x0b, "NIVEL.MOV") };
        let mut r = ByteReader::new(b, off);
        let ok = r.header().map(|h| h.kind == file_type::MOV).unwrap_or(false);
        if !ok {
            self.fatal(0x0b, "NIVEL.MOV");
        }
        let count = r.u16().unwrap_or(0) as i32;
        if n < 0 || n >= count {
            self.fatal(0x75, &n.to_string());
        }
        let e = off + 32 + n as usize * 0x1a0;
        let entry: Vec<u8> = (0..0x1a0).map(|i| b.get(e + i).copied().unwrap_or(0)).collect();
        let m = &mut self.mem;
        m.set16(p::MOV_W, entry[0] as i32 | (entry[1] as i32) << 8);
        m.set16(p::MOV_H, entry[2] as i32 | (entry[3] as i32) << 8);
        for (i, addr) in
            [0xcf, 0xd0, 0xd1, 0xd3, 0xd4, 0xd2, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb].into_iter().enumerate()
        {
            m.set8(addr, entry[4 + i] as i32);
        }
        m.write(p::POINTS, &entry[16..416]);
    }

    fn new_mouse(&mut self, n: i32) {
        if n < 0 || n as usize >= self.mou_file.count {
            self.fatal(0x74, &n.to_string());
        }
        if self.v102 {
            let rec = self.mou_file.record_v102(n as usize);
            self.mem.write(mo::TABLE, &rec[..0x48]);
            self.mem.write(v102::CURSORS_6_9, &rec[0x48..]);
        } else {
            let rec = self.mou_file.record(n as usize);
            self.mem.write(mo::TABLE, &rec);
        }
        self.mouse_action(0);
    }

    fn load_pcx(&mut self, file: i32, kind: i32) {
        let (buf, pal_addr) = match kind {
            0 => (Some(Buffer::Bg), pal::ESC),
            1 => (Some(Buffer::Dib), pal::DIB),
            2 => (Some(Buffer::Movs), pal::MOVS),
            3 => (None, pal::INV),
            _ => return,
        };
        if !(0..20).contains(&file) {
            return;
        }
        let name = file_entry(&self.mem, file as usize).name;
        match buf {
            Some(b) => self.load_picture(&name, b, pal_addr, 0x6e),
            None => {
                let bytes = self.lookup_file(&name).unwrap_or_else(|| self.fatal(0x6e, &crate::cp850_to_string(&name)));
                let img = decode_pcx(&bytes).unwrap_or_else(|e| self.fail(e));
                self.mem.write(pal_addr, &img.palette);
            }
        }
        self.merge_palettes();
        let (k, f) = (kind as usize, file as usize);
        let n = self.mem.slice(ds::FILE_NAMES + f * 13, 13);
        self.mem.write(ds::GFX_NAMES + k * 13, &n);
        let t = self.mem.slice(ds::FILE_TAB + f * 8, 8);
        self.mem.write(ds::GFX_TAB + k * 8, &t);
    }

    fn load_rgb(&mut self, file: i32, kind: i32) {
        let pal_addr = match kind {
            0 => pal::ESC,
            1 => pal::DIB,
            2 => pal::MOVS,
            3 => pal::INV,
            _ => self.fatal(0x6e, &self.file_name(file)),
        };
        let bytes = self.file_bytes(file).unwrap_or_else(|| self.fatal(0x6e, &self.file_name(file)));
        let mut pal6 = bytes;
        pal6.resize(768, 0);
        self.mem.write(pal_addr, &pal6);
        self.merge_palettes();
    }

    fn load_data(&mut self, file: i32, kind: i32) {
        if !(0..20).contains(&file) {
            return;
        }
        let f = file_entry(&self.mem, file as usize);
        let Some((mut bytes, mut offset)) = self.game.data_file(&f) else { self.fatal(0x68, &self.file_name(file)) };
        // inside EDGAME.DAT a data file may be stored like other resources (+5)
        if bytes.get(offset + 3).copied() == Some((kind + 15) as u8) {
            bytes = self.game.resource(&f).unwrap_or_default();
            offset = 0;
        }
        if let Err(e) = load_data_file(&mut self.mem, &bytes, offset, kind.max(0) as usize) {
            self.fail(e);
        }
    }

    fn play_cmf(&mut self, file: i32) {
        let name = if (0..20).contains(&file) { file_entry(&self.mem, file as usize).name } else { Vec::new() };
        if self.music_name.as_deref() == Some(&name[..]) && self.mem.u8(g::MUSIC_ON) == 1 {
            return;
        }
        // In EDGAME.DAT a FILE slot with no file has offset and size 0: the
        // original's loadfile (0xc240) then reads nothing and succeeds, so the
        // tune already in the CMF buffer starts again (El Dios de Nauru relies
        // on this). Loose files are opened by name and fail.
        let empty = self.game.is_packed() && (0..20).contains(&file) && file_entry(&self.mem, file as usize).size == 0;
        if !empty {
            self.cmf = self.file_bytes(file).unwrap_or_else(|| self.fatal(0x68, &self.file_name(file)));
        }
        self.music_name = Some(name);
        let _ = self.mixer.lock().unwrap().play_music(&self.cmf);
        self.mem.set8(g::MUSIC_ON, 1);
    }

    fn stop_cmf(&mut self) {
        self.mixer.lock().unwrap().stop_music();
        self.mem.set8(g::MUSIC_ON, 0);
        self.music_name = None;
    }

    /// SHOWPCX(file, mode, secs): show a picture, then return to the game.
    fn show_pcx(&mut self, file: i32, mut mode: i32, secs: i32) {
        self.mem.set8(g::PAUSED, 1);
        self.hide_cursor();
        let fx;
        if mode < 10 {
            self.fade_out();
            fx = 0;
        } else if mode < 20 {
            self.vga.fill(0);
            fx = 1;
            mode -= 10;
        } else {
            fx = 2;
            mode -= 20;
        }
        let bytes = self.file_bytes(file).unwrap_or_else(|| self.fatal(0x68, &self.file_name(file)));
        let img = decode_pcx(&bytes).unwrap_or_else(|e| self.fail(e));
        let n = img.pixels.len().min(64000);
        self.vga[..n].copy_from_slice(&img.pixels[..n]);
        let pal6: Vec<u8> = img.palette.iter().map(|v| v >> 2).collect();
        self.fade_to(&pal6);
        self.mem.set8(g::KEY, 0);
        let ms = secs as f64 * 1000.0;
        let start = self.clock();
        let any_input = |e: &mut Self| {
            let (l, r) = (e.button(0), e.button(1));
            l || r || e.key() != 0
        };
        match mode {
            0 => {
                while !any_input(self) {
                    self.wait(PASS_MS);
                }
            }
            1 => {
                while self.clock() - start < ms {
                    self.wait(PASS_MS);
                }
            }
            2 => {
                while !any_input(self) && self.clock() - start < ms {
                    self.wait(PASS_MS);
                }
            }
            3 => {
                while self.clock() - start < ms {
                    self.wait(PASS_MS);
                }
                while !any_input(self) {
                    self.wait(PASS_MS);
                }
            }
            _ => {}
        }
        if fx == 0 {
            self.fade_out();
        }
        if fx == 1 {
            self.vga.fill(0);
        }
        self.compose();
        let cur = self.mem.slice(pal::CUR, 768);
        if fx == 0 {
            self.fade_to(&cur);
        }
        self.set_dac(&cur);
        self.mem.set8(g::KEY, 0);
        self.mem.set8(g::PAUSED, 0);
        self.show_cursor();
    }

    /// PLAYFLI(file, mode, secs): play an Autodesk FLI animation.
    fn play_fli(&mut self, file: i32, mode: i32, secs: i32) {
        self.hide_cursor();
        self.vga.fill(0);
        let name = self.file_name(file);
        let bytes = self.file_bytes(file).unwrap_or_else(|| self.fatal(0x67, &name));
        let spd = bytes.get(16).copied().unwrap_or(0) as i32 | (bytes.get(17).copied().unwrap_or(0) as i32) << 8;
        let mut dec = FliDecoder::new(bytes)
            .unwrap_or_else(|_| self.fail(Error::new(format!("El archivo '{name}' no es un .FLI"))));
        let frame_ms = ((spd * 50 / 70) + (spd > 0) as i32) as f64 * TICK_MS;
        let (start, total) = (self.clock(), secs as f64 * 1000.0);
        self.mem.set8(g::KEY, 0);
        'outer: loop {
            dec.rewind();
            for _ in 0..dec.frames {
                let k = self.key();
                if [3, 6, 10].contains(&mode) && k == 0x39
                    || [2, 5, 9].contains(&mode) && k == 1
                    || [1, 4, 8].contains(&mode) && k != 0
                {
                    break 'outer;
                }
                self.mem.set8(g::KEY, 0);
                if !dec.next_frame() {
                    break;
                }
                let n = dec.pixels.len().min(64000);
                self.vga[..n].copy_from_slice(&dec.pixels[..n]);
                for (d, s) in self.dac.iter_mut().zip(&dec.palette) {
                    *d = s >> 2;
                }
                self.wait(if frame_ms > 0.0 { frame_ms } else { TICK_MS });
            }
            if mode == 11 {
                while self.clock() - start < total {
                    self.wait(PASS_MS);
                }
            }
            if (7..=10).contains(&mode) && self.clock() - start < total || (4..=6).contains(&mode) {
                continue;
            }
            break;
        }
        self.vga.fill(0);
        let cur = self.mem.slice(pal::CUR, 768);
        self.set_dac(&cur);
        self.compose();
        self.show_cursor();
        self.mem.set8(g::KEY, 0);
    }

    // ------------------------------------------------------------ save / load

    pub fn save_key(&self, slot: usize) -> String {
        let title: String =
            crate::cp850_to_string(&self.title).chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
        format!("{}/SAVEGAME._{}_", title.trim(), slot + 1)
    }

    pub fn save_state(&mut self, slot: usize, name: &[u8]) {
        let mut out = b"EDPKSAVE1\0".to_vec();
        let put = |out: &mut Vec<u8>, b: &[u8]| {
            out.extend((b.len() as u32).to_le_bytes());
            out.extend(b);
        };
        put(&mut out, &self.title);
        put(&mut out, name);
        put(&mut out, self.music_name.as_deref().unwrap_or(&[]));
        put(&mut out, &self.mem.bytes);
        let code: Vec<u8> = self.code.iter().flat_map(|w| w.to_le_bytes()).collect();
        put(&mut out, &code);
        let key = self.save_key(slot);
        self.storage.set(&key, &out);
    }

    fn read_save(&self, slot: usize) -> Option<SaveData> {
        let raw = self.storage.get(&self.save_key(slot))?;
        let rest = raw.strip_prefix(b"EDPKSAVE1\0")?;
        let mut parts = Vec::new();
        let mut p = 0;
        for _ in 0..5 {
            let n = u32::from_le_bytes(rest.get(p..p + 4)?.try_into().ok()?) as usize;
            parts.push(rest.get(p + 4..p + 4 + n)?.to_vec());
            p += 4 + n;
        }
        if parts[0] != self.title || parts[3].len() != self.mem.bytes.len() {
            return None;
        }
        let code = parts[4].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        Some(SaveData { name: parts[1].clone(), music: parts[2].clone(), mem: parts[3].clone(), code })
    }

    /// The name of the game saved in a slot, if any.
    pub fn save_name(&self, slot: usize) -> Option<Vec<u8>> {
        self.read_save(slot).map(|s| s.name)
    }

    pub fn load_state(&mut self, slot: usize) -> bool {
        let Some(d) = self.read_save(slot) else { return false };
        self.mem.bytes.copy_from_slice(&d.mem);
        self.code = d.code;
        let esc = self.mem.cstr(ds::GFX_NAMES, 13);
        self.load_picture(&esc, Buffer::Bg, pal::ESC, 0x67);
        for i in 0..25 {
            let a = ds::DRAWANIM_TAB + i * 20;
            if self.mem.u8(a + 0xd) == 0 {
                continue;
            }
            let name = self.mem.cstr(a, 13);
            self.load_picture(&name, Buffer::Dib, pal::DIB, 0x66);
            let m = &self.mem;
            let c = ds::COORANIM + m.i16(a + 0xe).clamp(0, 99) as usize * 10;
            blit(
                &mut self.bg,
                &self.dib,
                m.i16(c),
                m.i16(c + 2),
                m.i16(a + 0x10),
                m.i16(a + 0x12),
                m.i16(c + 4),
                m.i16(c + 6),
                false,
                200,
            );
        }
        let dib = self.mem.cstr(ds::GFX_NAMES + 13, 13);
        self.load_picture(&dib, Buffer::Dib, pal::DIB, 0x66);
        let movs = self.mem.cstr(ds::GFX_NAMES + 26, 13);
        self.load_picture(&movs, Buffer::Movs, pal::MOVS, 0x6c);
        for s in 0..30 {
            let a = inv::SLOTS + s * 6;
            if self.mem.u8(a) == 1 {
                self.mem.set8(a, 0);
                let o = self.mem.i16(a + 2);
                self.add_inv(o);
            }
        }
        self.mixer.lock().unwrap().stop_music();
        self.music_name = None;
        if !d.music.is_empty()
            && self.mem.u8(g::MUSIC_ON) == 1
            && let Some(i) = (0..20).find(|&i| file_entry(&self.mem, i).name == d.music)
        {
            self.mem.set8(g::MUSIC_ON, 0);
            self.play_cmf(i as i32);
        }
        self.mem.set8(st::HL, 0xff);
        self.compose();
        true
    }
}

struct SaveData {
    name: Vec<u8>,
    music: Vec<u8>,
    mem: Vec<u8>,
    code: Vec<i16>,
}

#[derive(Clone, Copy)]
enum Buffer {
    Bg,
    Dib,
    Movs,
}

/// 0x34a: rectangular blit with colour-0 transparency and optional mirroring.
#[allow(clippy::too_many_arguments)]
pub fn blit(dst: &mut [u8], src: &[u8], sx: i32, sy: i32, dx: i32, dy: i32, w: i32, h: i32, mirror: bool, maxy: i32) {
    for r in 0..h.max(0) {
        let (syy, dyy) = (sy + r, dy + r);
        if syy < 0 || syy > maxy || syy >= 200 || dyy < 0 || dyy > maxy || dyy >= 200 {
            continue;
        }
        for c in 0..w.max(0) {
            let sxx = if mirror { sx + w - 1 - c } else { sx + c };
            let dxx = dx + c;
            if !(0..320).contains(&sxx) || !(0..320).contains(&dxx) {
                continue;
            }
            let v = src[(syy * 320 + sxx) as usize];
            if v != 0 {
                dst[(dyy * 320 + dxx) as usize] = v;
            }
        }
    }
}

/// Host-side handle to a v2 engine running on its own thread.
pub type EngineHandle = host::EngineHandle<Engine>;

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
        self.on_input(ev)
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
