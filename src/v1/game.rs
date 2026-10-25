//! RUNVGA v1 main loop (0x5037), timer interrupt, input, click actions,
//! texts, the label line and the screen composition.
//! See docs/v1-runvga/03-mainloop-ui.md and 04-player-render.md.

use super::data::{self, THING_SIZE, ds};
use super::engine::{Engine, RETRACE_MS, g};
use super::player as p;

/// Text line slots: 5 x 0x66 bytes {active, drawn, x, y, .., w@+0xa, h@+0xc, text@+0x12, colour@+0x65}.
pub const LINES: usize = 0x8fdb;
pub const LINE_SIZE: usize = 0x66;
pub const SPEECH_TIMER: usize = 0x903e;
/// Label line: state, last drawn state, rect (x 353f, y 3541, w 3548, h 354a), old rect, text 0x3550.
pub mod label {
    pub const STATE: usize = 0x353d;
    pub const DRAWN: usize = 0x353e;
    pub const X: usize = 0x353f;
    pub const Y: usize = 0x3541;
    pub const OLD_X: usize = 0x3543;
    pub const OLD_Y: usize = 0x3545;
    pub const THING: usize = 0x3547;
    pub const W: usize = 0x3548;
    pub const H: usize = 0x354a;
    pub const OLD_W: usize = 0x354c;
    pub const OLD_H: usize = 0x354e;
    pub const TEXT: usize = 0x3550;
    pub const COLOUR: usize = 0x3539;
    pub const BORDER: usize = 0x353b;
}
/// Held-object state.
pub mod held {
    pub const STATE: usize = 0x35a0;
    pub const SLOT: usize = 0x35a1;
    pub const OBJ: usize = 0x35a2;
    pub const HOVER_OBJ: usize = 0x35a3;
    pub const PENDING: usize = 0x35a4;
    pub const TARGET_KIND: usize = 0x35a5;
    pub const TARGET: usize = 0x35a6;
    pub const ACTION: usize = 0x35a7;
}
pub const HOVER_INV: usize = 0x9280;
pub const USE_THING: usize = 0x92ab;
/// While composing ([0x94] = 1) the ISR leaves the mouse alone.
pub const COMPOSING: usize = 0x94;

fn thing(t: usize) -> usize {
    ds::THINGS + t * THING_SIZE
}

fn slot(s: usize) -> usize {
    g::INV + s * g::INV_SIZE
}

/// The "record" of 1-based hover index h, as the original addresses it
/// (0x3587 + 0x22*h): h = 0 reads just before slot 0.
fn hover_rec(h: i32) -> usize {
    (0x3587 + 0x22 * h) as usize
}

impl Engine {
    // ------------------------------------------------------------ timer

    /// The 50 Hz timer interrupt (0x36ee).
    pub(super) fn timer_tick(&mut self) {
        let m = &mut self.mem;
        if m.u8(COMPOSING) != 1 {
            let (w, h) = (m.i16(g::CURSOR_W), m.i16(g::CURSOR_H));
            let x = (self.pointer.0 - m.i16(g::HOT_X)).clamp(0, (319 - w).max(0));
            let y = (self.pointer.1 - m.i16(g::HOT_Y)).clamp(0, (200 - h).max(0));
            m.set16(g::MOUSE_X, x);
            m.set16(g::MOUSE_Y, y);
        }
        let mode = m.i16(g::CURSOR_STATE);
        if mode == 2 || mode == 4 {
            m.set16(g::CURSOR_STATE, 3);
            m.set16(0x92b2, 3);
        }
        if m.u8(g::PAUSED) == 0 {
            if m.u8(g::TEXT_ACTIVE) == 1 && m.i16(SPEECH_TIMER) > 0 {
                m.set16(SPEECH_TIMER, m.i16(SPEECH_TIMER) - 1);
            }
            for a in [p::STEP_WAIT, g::ANIM_WAIT] {
                if m.u8(a) > 0 {
                    m.set8(a, m.u8(a) - 1);
                }
            }
        }
        if m.i16(g::COUNTDOWN) > 0 {
            m.set16(g::COUNTDOWN, m.i16(g::COUNTDOWN) - 1);
        }
    }

    /// Click point = cursor top-left + hotspot.
    pub fn hotspot(&self) -> (i32, i32) {
        let m = &self.mem;
        (m.i16(g::MOUSE_X) + m.i16(g::HOT_X), m.i16(g::MOUSE_Y) + m.i16(g::HOT_Y))
    }

    /// Screen + software cursor -> `screen`; DAC -> 8-bit palette.
    pub fn present(&mut self) {
        self.screen.copy_from_slice(&self.vga);
        let m = &self.mem;
        let mode = m.i16(g::CURSOR_STATE);
        if mode == 0 || mode == 1 {
            let (cx, cy) = (m.i16(g::MOUSE_X), m.i16(g::MOUSE_Y));
            let (sx, sy) = (m.i16(g::CURSOR_SRCX), m.i16(g::CURSOR_SRCY));
            for y in 0..m.i16(g::CURSOR_H) {
                for x in 0..m.i16(g::CURSOR_W) {
                    let (dx, dy, px, py) = (cx + x, cy + y, sx + x, sy + y);
                    if (0..320).contains(&dx)
                        && (0..200).contains(&dy)
                        && (0..320).contains(&px)
                        && (0..200).contains(&py)
                    {
                        let c = self.sheets[2][(py * 320 + px) as usize];
                        if c != 0 {
                            self.screen[(dy * 320 + dx) as usize] = c;
                        }
                    }
                }
            }
        }
        for i in 0..768 {
            let v = self.dac[i] & 63;
            self.palette[i] = (v << 2) | (v >> 4);
        }
    }

    // ------------------------------------------------------------ main loop

    pub(super) fn main_loop(&mut self) {
        loop {
            if let Some(slot) = self.pending_load.take() {
                self.load_state(slot);
            }
            if self.mem.i16(g::LEVEL) != self.mem.i16(g::PREV_LEVEL) {
                self.change_level();
                let l = self.mem.i16(g::LEVEL);
                self.mem.set16(g::PREV_LEVEL, l);
            }
            self.animations();
            self.dispatch();
            if self.mem.u8(g::TEXT_ACTIVE) == 1 {
                self.text_sequencer();
            }
            self.input();
            p::tick(&mut self.mem);
            if self.mem.u8(label::STATE) == 2 {
                self.build_label();
            }
            self.auto_layer();
            if self.mem.u8(g::KEY) == 0x39 {
                self.mem.set8(g::PAUSED, 1);
                self.menu();
                self.mem.set8(g::KEY, 0);
                self.mem.set8(g::PAUSED, 0);
            }
            let a = self.mem.u8(g::ACTION);
            if (a > 0 && a < 50) || self.mem.u8(g::TEXT_ACTIVE) == 1 {
                self.mem.set8(label::STATE, 0);
                self.mem.set8(label::DRAWN, 0xff);
                self.mem.set8(label::THING, 0);
            }
            self.compose_pass();
            if self.quit_keys() {
                return;
            }
        }
    }

    /// Ctrl + Esc held together quits.
    fn quit_keys(&self) -> bool {
        self.keys[1] != 0 && self.keys[0x1d] != 0
    }

    /// 0x7d26: animation scripts, when their countdown allows.
    fn animations(&mut self) {
        if self.mem.u8(g::ANIM_WAIT) != 0 {
            return;
        }
        for i in 0..50 {
            let a = ds::ANIMS + i * 10;
            if self.mem.i16(a) == 1 {
                let base = self.mem.i16(a + 2);
                if self.run_script(base, 0xfff0, a + 4, a + 6) == 0 {
                    self.mem.set16(a + 4, 0);
                    self.mem.set16(a, 0);
                }
            }
        }
        let t = self.mem.u8(g::ANIM_TICKS);
        self.mem.set8(g::ANIM_WAIT, t);
    }

    /// Run ACTION 0 to completion, as level changes do (each yield cancels
    /// walking and speech and resumes at once).
    pub(super) fn run_to_completion(&mut self, base: i32) {
        let mut spins = 0;
        loop {
            if self.run_script(base, g::ACTION, g::ACTION_PC, g::ACTION_FLAG) == 0 {
                return;
            }
            self.mem.set8(p::WALKING, 0);
            self.mem.set8(g::TEXT_ACTIVE, 0);
            // The original spins without letting time pass; give the host a
            // chance now and then so a script stuck in a RET loop stays
            // interruptible.
            spins += 1;
            if spins % 10000 == 0 {
                self.wait(RETRACE_MS);
            }
        }
    }

    /// 0x11c9: change to level [0x927c].
    pub(super) fn change_level(&mut self) {
        loop {
            self.mem.set16(g::CURSOR_STATE, 2);
            let n = self.mem.i16(g::LEVEL);
            let names = self.mem.slice(ds::GFX_NAMES + 26, 26);
            let tabs = self.mem.slice(ds::GFX_TAB + 16, 16);
            if !(0..998).contains(&n) {
                self.fatal(0x65, &n.to_string());
            }
            match data::load_level(&mut self.mem, &self.src, n as usize) {
                Ok(code) => self.code = code,
                Err(_) => self.fatal(0x65, &n.to_string()),
            }
            if n == 0 {
                self.mem.set8(g::ACTION, 0);
                self.mem.set8(g::ACTION_PC, 0);
                let base = self.mem.i16(ds::ACTION_OFFS);
                self.run_to_completion(base);
                if self.mem.u8(ds::GFX_NAMES + 26) == 0 {
                    self.fatal(0x6a, "");
                }
                if self.mem.u8(ds::GFX_NAMES + 39) == 0 {
                    self.fatal(0x6b, "");
                }
                self.load_sheet(2, 0x6c);
                self.load_sheet(3, 0x6d);
                if self.mem.i16(g::LEVEL) == 0 {
                    self.fatal(0x69, "");
                }
                continue;
            }
            for t in 0..30 {
                let a = thing(t);
                for (from, to) in [(0x10, 0x14), (0x12, 0x16), (0x18, 0x1c), (0x1a, 0x1e)] {
                    let v = self.mem.i16(a + from);
                    self.mem.set16(a + to, v);
                }
            }
            self.load_sheet(1, 0x66);
            self.load_sheet(0, 0x67);
            // gfx 2 and 3 are global: set by level 0, kept across later levels
            self.mem.write(ds::GFX_NAMES + 26, &names);
            self.mem.write(ds::GFX_TAB + 16, &tabs);
            self.merge_palette();
            self.fade_out();
            self.mem.set8(g::ACTION, 0);
            self.mem.set8(g::ACTION_PC, 0);
            let base = self.mem.i16(ds::ACTION_OFFS);
            if base != 0 {
                self.run_to_completion(base);
            }
            self.place_player();
            self.mem.set8(g::ACTION_PC, 0);
            self.full_redraw();
            self.inventory_arrows();
            self.present_all();
            let pal = self.mem.slice(g::PAL_SCENE, 768);
            self.fade_in(&pal);
            self.mem.set16(g::CURSOR_STATE, 0);
            self.mem.set8(g::KEY, 0);
            return;
        }
    }

    /// Put the player at door [0x92a3] on level entry.
    fn place_player(&mut self) {
        let m = &mut self.mem;
        let d = ds::DOORS + 10 * m.u8(g::DOOR) as usize;
        let (fw, fh) = (m.i16(p::FRAME_W), m.i16(p::FRAME_H));
        m.set16(p::PW, fw);
        m.set16(p::PH, fh);
        if m.i16(d) != 0 {
            let (x, y, dir) = (m.i16(d + 2), m.i16(d + 4), m.i16(d + 6));
            if x != -100 {
                m.set16(p::PX, x - fw / 2);
            }
            if y != -100 {
                m.set16(p::PY, y - fh);
            }
            if dir != -100 {
                m.set8(p::LAST_FRAME, m.u8(p::FRAME));
                p::face(m, dir);
            }
            let f = m.u8(p::FRAME) as usize;
            m.set16(p::SRCX, m.i16(p::FRAMES + 4 * f));
            m.set16(p::SRCY, m.i16(p::FRAMES + 4 * f + 2));
            m.set8(g::ACTION, m.u8(d + 8));
        } else {
            m.set8(p::FRAME, m.u8(p::S_SIDE));
            let f = m.u8(p::FRAME) as usize;
            m.set16(p::SRCX, m.i16(p::FRAMES + 4 * f));
            // QUIRK: the original writes the source y over the source x.
            m.set16(p::SRCX, m.i16(p::FRAMES + 4 * f + 2));
            m.set8(g::ACTION, 0);
        }
    }

    /// 0x4f83: things in layers 0/1 go in front of the player if their bottom
    /// is at or below his feet.
    fn auto_layer(&mut self) {
        let feet = self.mem.i16(p::PY) + self.mem.i16(p::FRAME_H);
        for t in 0..30 {
            let a = thing(t);
            let layer = self.mem.i16(a + 0x0c);
            if self.mem.i16(a + 0x22) != 0 && layer != 2 && layer != 3 {
                let front = self.mem.i16(a + 0x12) + self.mem.i16(a + 0x1a) >= feet;
                self.mem.set16(a + 0x0c, front as i32);
            }
        }
    }

    // ------------------------------------------------------------ click actions

    /// Face the player as the thing's direction field says.
    fn face_thing(&mut self, t: usize) {
        let dir = self.mem.i16(thing(t) + 0x39);
        p::face(&mut self.mem, dir);
    }

    /// 0x764e: the pseudo-actions 100..104 and the current action's script.
    fn dispatch(&mut self) {
        let a = self.mem.u8(g::ACTION);
        let sub = self.mem.u8(g::ACTION_PC);
        let walking = self.mem.u8(p::WALKING) != 0;
        let target = self.mem.u8(held::OBJ) as usize;
        let walk_to = |e: &mut Engine, t: usize| {
            let (x, y) = (e.mem.i16(thing(t) + 0x35), e.mem.i16(thing(t) + 0x37));
            p::go_xy(&mut e.mem, x, y, e.limit_stride);
            e.mem.set8(g::ACTION_PC, 1);
        };
        match a {
            100 | 104 | 101 => match sub {
                0 => walk_to(self, target),
                1 if !walking => {
                    self.face_thing(target);
                    self.mem.set8(g::ACTION_PC, 2);
                }
                2 => match a {
                    100 => {
                        let text = self.mem.i16(thing(target) + 0x43);
                        self.say(&[text]);
                        self.mem.set8(g::ACTION, 0);
                    }
                    104 => {
                        let act = self.mem.u8(thing(target) + 0x41);
                        self.mem.set8(g::ACTION, act);
                        self.mem.set8(g::ACTION_PC, 0);
                    }
                    _ => {
                        let take = self.mem.i16(thing(target) + 0x3d);
                        if take != 0 {
                            self.mem.set8(g::ACTION, take & 0xff);
                            self.mem.set8(g::ACTION_PC, 0);
                        } else {
                            self.say(&[1]);
                            self.mem.set8(g::ACTION, 0);
                        }
                    }
                },
                _ => {}
            },
            102 => {
                let t = self.mem.u8(USE_THING) as usize;
                match sub {
                    0 => walk_to(self, t),
                    1 if !walking => {
                        self.face_thing(t);
                        self.mem.set8(g::ACTION_PC, 3);
                    }
                    3 => self.use_result(),
                    _ => {}
                }
            }
            103 => {
                if sub == 0 {
                    self.use_result();
                }
            }
            _ => {
                if a != 0 && self.mem.u8(g::TEXT_ACTIVE) == 0 && !walking {
                    let base = self.mem.i16(ds::ACTION_OFFS + 2 * a as usize);
                    if self.run_script(base, g::ACTION, g::ACTION_PC, g::ACTION_FLAG) == 0 {
                        self.mem.set8(g::ACTION, 0);
                        self.mem.set8(g::ACTION_PC, 0);
                    }
                }
            }
        }
    }

    /// End of "use object on thing": run the table's action or say message 2.
    fn use_result(&mut self) {
        self.mem.set8(held::STATE, 0);
        let act = self.mem.u8(held::ACTION);
        if act != 0 {
            self.mem.set8(g::ACTION, act);
            self.mem.set8(g::ACTION_PC, 0);
        } else {
            self.say(&[2]);
            self.mem.set8(g::ACTION, 0);
        }
    }

    /// 0x7c99: left click on the hovered thing, by its type.
    fn thing_click(&mut self) {
        let t = self.mem.u8(HOVER_THING) - 1;
        self.mem.set8(held::OBJ, t);
        let a = thing(t.max(0) as usize);
        match self.mem.i16(a + 0x3f) {
            0 => self.start_action(100, 0),
            1 => self.start_action(100, 2),
            2 => self.start_action(104, 0),
            3 => {
                let act = self.mem.u8(a + 0x41);
                self.start_action(act, 0);
            }
            _ => {}
        }
    }

    fn start_action(&mut self, a: i32, pc: i32) {
        self.mem.set8(g::ACTION, a);
        self.mem.set8(g::ACTION_PC, pc);
    }

    /// 0x7c02: held object used on thing [0x35a6].
    fn use_on_thing(&mut self) {
        let (obj, target) = (self.mem.u8(held::OBJ), self.mem.u8(held::TARGET));
        let found = (0..30)
            .map(|i| ds::ACINVTH + 8 * i)
            .find(|&r| self.mem.i16(r) != 0 && self.mem.i16(r + 2) == obj && self.mem.i16(r + 4) == target);
        match found {
            Some(r) => {
                match self.mem.i16(r) {
                    1 => self.mem.set8(g::ACTION, 102),
                    2 => self.mem.set8(g::ACTION, 103),
                    _ => {}
                }
                let v = self.mem.u8(r + 6);
                self.mem.set8(held::ACTION, v);
            }
            None => {
                self.mem.set8(held::ACTION, 0);
                self.mem.set8(g::ACTION, 102);
            }
        }
        self.mem.set8(g::ACTION_PC, 0);
        self.mem.set8(USE_THING, target);
        self.mem.set8(held::PENDING, 0);
    }

    /// 0x7a9a: held object combined with another inventory object.
    fn combine(&mut self) {
        let (a, b) = (self.mem.u8(held::OBJ), self.mem.u8(held::TARGET));
        for i in 0..50 {
            let r = ds::ACINVINV + 5 * i;
            if self.mem.u8(r) == 0 {
                continue;
            }
            let (o1, o2) = (self.mem.u8(r + 1), self.mem.u8(r + 2));
            if !((a == o1 && b == o2) || (a == o2 && b == o1)) {
                continue;
            }
            match self.mem.u8(r + 4) {
                0 => {
                    self.sub_inv(o1);
                    self.sub_inv(o2);
                }
                1 => self.sub_inv(o1),
                2 => self.sub_inv(o2),
                _ => {}
            }
            let new = self.mem.u8(r + 3);
            self.add_inv(new);
            self.say(&[4]);
            return;
        }
        self.say(&[3]);
    }

    // ------------------------------------------------------------ input

    /// 0x67a1: which inventory slot is under the cursor.
    fn inventory_hover(&mut self) {
        let (px, py) = self.hotspot();
        let scroll = self.mem.u8(g::INV_SCROLL);
        for i in 0..8 {
            if 13 + 37 * i > px || 48 + 37 * i < px || py < 173 {
                continue;
            }
            if self.mem.u8(slot((scroll + i) as usize) + 0x1e) == 0 {
                continue;
            }
            let h = scroll + i + 1;
            self.mem.set16(HOVER_INV, h);
            if self.mem.u8(held::STATE) != 0 && self.mem.u8(hover_rec(h) + 0x20) == self.mem.u8(held::OBJ) {
                self.mem.set16(HOVER_INV, 0);
                self.mem.set8(held::HOVER_OBJ, 0xff);
            }
            let h = self.mem.i16(HOVER_INV);
            if self.mem.u8(hover_rec(h) + 0x20) == self.mem.u8(held::HOVER_OBJ) && self.mem.u8(label::STATE) != 0 {
                return;
            }
            self.mem.set8(label::STATE, 2);
            return;
        }
        if self.mem.u8(label::THING) == 0 && self.mem.u8(held::STATE) == 0 {
            self.mem.set8(label::STATE, 0);
        }
        if self.mem.u8(held::STATE) != 0 && self.mem.u8(held::HOVER_OBJ) != 0xff {
            self.build_label();
        }
        self.mem.set8(held::HOVER_OBJ, 0xff);
    }

    /// 0x4c44: which thing is under the cursor (index order, first wins).
    fn thing_hover(&mut self) {
        let (px, py) = self.hotspot();
        for t in 0..30 {
            let a = thing(t);
            let m = &self.mem;
            let (x, y, w, h, st) =
                (m.i16(a + 0x10), m.i16(a + 0x12), m.i16(a + 0x18), m.i16(a + 0x1a), m.i16(a + 0x22));
            if x <= px && px <= x + w && y <= py && py <= y + h && (st == 1 || st == 3) {
                self.mem.set8(HOVER_THING, t as i32 + 1);
                if self.mem.u8(label::THING) != t as i32 + 1 {
                    self.mem.set8(label::STATE, 2);
                }
                return;
            }
        }
        if self.mem.u8(held::STATE) == 0 && self.mem.i16(HOVER_INV) == 0 {
            self.mem.set8(label::STATE, 0);
        }
        if self.mem.u8(held::STATE) != 0 && self.mem.u8(label::THING) != 0 {
            self.mem.set8(label::STATE, 2);
        }
        self.mem.set8(label::THING, 0);
    }

    /// The part common to both buttons (cancel, stop, drop, walk, target).
    fn common_click(&mut self) {
        let speaking = self.mem.u8(g::TEXT_ACTIVE) != 0;
        if !speaking && self.mem.u8(g::ACTION) != 0 && self.mem.i16(g::CURSOR_STATE) != 2 {
            self.mem.set8(g::ACTION, 0);
        }
        p::stop(&mut self.mem);
        let hover_inv = self.mem.i16(HOVER_INV);
        let hover_thing = self.mem.u8(HOVER_THING);
        if self.mem.u8(held::STATE) != 0 && !speaking && hover_inv == 0 && hover_thing == 0 {
            self.mem.set8(held::STATE, 0);
        }
        let (px, py) = self.hotspot();
        if self.mem.u8(held::STATE) != 1 && self.mem.u8(g::ACTION) == 0 && hover_thing == 0 && py < 0x94 {
            p::go_xy(&mut self.mem, px, py, self.limit_stride);
        }
        if self.mem.u8(held::STATE) != 0 && !speaking {
            if hover_inv != 0 {
                self.mem.set8(held::TARGET_KIND, hover_inv - 1);
                let o = self.mem.u8(hover_rec(hover_inv) + 0x20);
                self.mem.set8(held::TARGET, o);
                self.mem.set8(held::PENDING, 1);
                self.mem.set8(label::STATE, 2);
            }
            if hover_thing != 0 {
                self.mem.set8(held::TARGET_KIND, 100);
                self.mem.set8(held::TARGET, hover_thing - 1);
                self.mem.set8(held::PENDING, 1);
                self.mem.set8(label::STATE, 2);
            }
        }
    }

    /// 0x4842: mouse input, once per pass.
    fn input(&mut self) {
        let mode = self.mem.i16(g::CURSOR_STATE);
        if mode == 2 || mode == 3 {
            return;
        }
        if self.mem.u8(g::TEXT_ACTIVE) != 0 || mode != 0 {
            let l = self.button(0);
            let r = self.button(1);
            if l || r {
                self.end_text();
            }
            return;
        }
        let a = self.mem.u8(g::ACTION);
        if a != 0 && a < 50 {
            return;
        }
        self.mem.set16(g::CURSOR_STATE, 0);
        self.mem.set8(0x9200, 0);
        self.mem.set8(HOVER_THING, 0);
        self.mem.set16(HOVER_INV, 0);
        self.inventory_hover();
        self.thing_hover();
        if self.mem.u8(held::STATE) == 0xff && self.mem.u8(held::PENDING) == 0xff {
            self.mem.set8(held::STATE, 0);
            self.mem.set8(held::PENDING, 0);
            self.mem.set8(label::STATE, 2);
        }
        if self.mem.u8(held::STATE) == 1 && self.mem.u8(held::PENDING) == 1 {
            if self.mem.u8(held::TARGET_KIND) == 100 {
                self.use_on_thing();
                self.mem.set8(held::PENDING, 0);
            } else {
                self.combine();
                self.mem.set8(held::STATE, 0xff);
                self.mem.set8(held::PENDING, 0xff);
            }
        }
        if self.button(0) {
            let (px, py) = self.hotspot();
            if (0..=13).contains(&px) && py > 0xad {
                if self.mem.u8(g::INV_SCROLL) > 0 {
                    let s = self.mem.u8(g::INV_SCROLL) - 1;
                    self.mem.set8(g::INV_SCROLL, s);
                }
                self.inventory_arrows();
                return;
            }
            // QUIRK: the right arrow tests the hotspot's x offset for y.
            let qy = self.mem.i16(g::MOUSE_Y) + self.mem.i16(g::HOT_X);
            if (0x133..=0x140).contains(&px) && qy > 0xad {
                let s = self.mem.u8(g::INV_SCROLL) as usize;
                if self.mem.u8(slot(s + 8) + 0x1e) != 0 {
                    self.mem.set8(g::INV_SCROLL, s as i32 + 1);
                    self.inventory_arrows();
                    return;
                }
            }
            self.common_click();
            let speaking = self.mem.u8(g::TEXT_ACTIVE) != 0;
            if self.mem.u8(HOVER_THING) != 0 && self.mem.u8(held::STATE) != 1 && !speaking {
                self.thing_click();
            }
            let speaking = self.mem.u8(g::TEXT_ACTIVE) != 0;
            let h = self.mem.i16(HOVER_INV);
            if h != 0 && self.mem.u8(held::STATE) != 1 && !speaking {
                self.say(&[h + 0x31]);
            }
        }
        if !self.button(1) {
            return;
        }
        self.common_click();
        let speaking = self.mem.u8(g::TEXT_ACTIVE) != 0;
        let h = self.mem.i16(HOVER_INV);
        if h != 0 && self.mem.u8(held::STATE) != 1 && !speaking {
            self.mem.set8(held::SLOT, h - 1);
            let o = self.mem.u8(hover_rec(h) + 0x20);
            self.mem.set8(held::OBJ, o);
            self.mem.set8(held::STATE, 1);
            self.mem.set8(label::STATE, 2);
        }
        let t = self.mem.u8(HOVER_THING);
        if self.mem.u8(held::STATE) == 0 && t != 0 && self.mem.u8(g::TEXT_ACTIVE) == 0 {
            self.mem.set8(held::OBJ, t - 1);
            self.start_action(101, 0);
        }
    }

    // ------------------------------------------------------------ inventory bar

    /// 0x6c87: force the visible slots to redraw and light the scroll arrows
    /// (palette entries 248/249) when scrolling is possible.
    pub(super) fn inventory_arrows(&mut self) {
        let scroll = self.mem.u8(g::INV_SCROLL) as usize;
        for s in scroll..scroll + 8 {
            self.mem.set8(slot(s) + 0x1f, 0);
        }
        let bg = g::PAL_SLOT[0];
        let m = &mut self.mem;
        let dark = [m.u8(g::PAL_SCENE), m.u8(g::PAL_SCENE + 1), m.u8(bg + 2)];
        let left: [i32; 3] = if scroll > 0 { [m.u8(bg + 744), m.u8(bg + 745), m.u8(bg + 746)] } else { dark };
        let right: [i32; 3] =
            if m.u8(slot(scroll + 8) + 0x1e) != 0 { [m.u8(bg + 747), m.u8(bg + 748), m.u8(bg + 749)] } else { dark };
        for k in 0..3 {
            m.set8(g::PAL_SCENE + 744 + k, left[k]);
            m.set8(g::PAL_SCENE + 747 + k, right[k]);
        }
        m.set8(g::PAL_DIRTY, 1);
    }

    // ------------------------------------------------------------ texts

    /// 0x762b: all text lines off, the current text ends.
    pub(super) fn end_text(&mut self) {
        for i in 0..5 {
            self.mem.set8(LINES + LINE_SIZE * i, 0);
        }
        self.mem.set16(SPEECH_TIMER, 0);
    }

    /// 0x7513: show the queued texts one after another.
    fn text_sequencer(&mut self) {
        let m = &self.mem;
        if m.u8(g::TEXT_ACTIVE) == 1 && m.u8(g::TEXT_INDEX) != m.u8(g::TEXT_SHOWN) {
            let k = m.u8(g::TEXT_INDEX);
            let id = m.i16(g::TEXT_QUEUE + 2 * k as usize);
            if id == -1 || k > 9 {
                self.mem.set8(g::TEXT_ACTIVE, 0);
                if self.mem.u8(g::TEXT_BY_PLAYER) == 1 {
                    let m = &mut self.mem;
                    m.set8(g::TEXT_BY_PLAYER, 0);
                    m.set8(p::FRAME, m.u8(p::SAVED));
                    m.set16(0x91f3, -1);
                    let f = m.u8(p::FRAME) as usize;
                    m.set16(p::SRCX, m.i16(p::FRAMES + 4 * f));
                    m.set16(p::SRCY, m.i16(p::FRAMES + 4 * f + 2));
                    let (w, h) = (m.i16(p::FRAME_W), m.i16(p::FRAME_H));
                    m.set16(p::PW, w);
                    m.set16(p::PH, h);
                    m.set16(0x91ed, w);
                    m.set16(0x91ef, h);
                }
            } else {
                let text = self.mem.cstr((ds::TEXTS as i32 + 250 * id) as usize, 250);
                if self.mem.u8(g::TEXT_BY_PLAYER) == 1 {
                    let c = self.mem.u8(g::SPEECH_COLOUR);
                    self.layout_text(&text, c, None);
                } else {
                    let (x, y, c) = (self.mem.u8(g::PTEXT_X), self.mem.u8(g::PTEXT_Y), self.mem.u8(g::PTEXT_COLOUR));
                    self.layout_text(&text, c, Some((x, y)));
                }
                let k = self.mem.u8(g::TEXT_INDEX);
                self.mem.set8(g::TEXT_SHOWN, k);
            }
        }
        if self.mem.i16(SPEECH_TIMER) <= 0 && self.mem.u8(g::TEXT_ACTIVE) == 1 {
            self.end_text();
            let k = self.mem.u8(g::TEXT_INDEX);
            self.mem.set8(g::TEXT_INDEX, k + 1);
        }
    }

    /// 0x6d74 (player, `at` = None) / 0x70c3 (PTEXT at (x, y)): split the text
    /// at `^` into the line slots (first line in slot 4), centre, clamp.
    fn layout_text(&mut self, text: &[u8], colour: i32, at: Option<(i32, i32)>) {
        let line = |i: usize| LINES + LINE_SIZE * i;
        let ch = |c: usize| text.get(c).copied().unwrap_or(0);
        for i in 0..5 {
            self.mem.set8(line(i), 0);
            self.mem.set8(line(i) + 0x12, 0);
        }
        let (mut c, mut chars) = (0usize, 0i32);
        let mut si = 4i32;
        while si >= 0 {
            let l = line(si as usize);
            if self.mem.u8(l) == 0 {
                self.mem.set8(l, 1);
                self.mem.set16(l + 0xa, 0);
                self.mem.set16(l + 0xc, 10);
                self.mem.set8(l + 0x65, colour);
                let mut di = 0usize;
                while ch(c) != 0 && ch(c) != b'^' {
                    if di < 0x38 {
                        self.mem.set8(l + 0x12 + di, ch(c) as i32);
                        let w = self.mem.i16(l + 0xa) + self.font.size(ch(c)).0 - 1;
                        self.mem.set16(l + 0xa, w);
                        self.mem.set8(l + 0x13 + di, 0);
                        chars += 1;
                    }
                    c += 1;
                    di += 1;
                }
                if at.is_none() {
                    if ch(c) == b'^' {
                        c += 1;
                    }
                    if ch(c) == 0 {
                        si = -1;
                    }
                } else {
                    if ch(c) == 0 {
                        si = -1;
                    }
                    if ch(c) == b'^' {
                        c += 1;
                    }
                }
            }
            si -= 1;
        }
        // horizontal: centre each line, clamp the widest, shift the others alike
        let (mut widest, mut wi) = (0, 0usize);
        for i in 0..5 {
            let l = line(i);
            let w = self.mem.i16(l + 0xa);
            let centre = match at {
                None => self.mem.i16(p::PX) + self.mem.i16(p::PW) / 2,
                Some((x, _)) => x,
            };
            self.mem.set16(l + 2, centre - w / 2);
            if self.mem.u8(l) == 1 {
                let centre = match at {
                    None => self.mem.i16(p::PX) + self.mem.i16(p::FRAME_W) / 2,
                    Some((x, _)) => x,
                };
                self.mem.set16(l + 2, centre - w / 2);
                if w > widest {
                    widest = w;
                    wi = i;
                }
            }
        }
        let mut shift = 0;
        let lw = line(wi);
        while self.mem.i16(lw + 2) < 5 {
            self.mem.set16(lw + 2, self.mem.i16(lw + 2) + 1);
            shift += 1;
        }
        while self.mem.i16(lw + 2) + self.mem.i16(lw + 0xa) > 0x13b {
            self.mem.set16(lw + 2, self.mem.i16(lw + 2) - 1);
            shift -= 1;
        }
        for i in 0..5 {
            if self.mem.u8(line(i)) == 1 && i != wi {
                self.mem.set16(line(i) + 2, self.mem.i16(line(i) + 2) + shift);
            }
        }
        // vertical: the lowest active slot sits above the head, the others stack up
        let base_y = match at {
            None => self.mem.i16(p::PY) - 11,
            Some((_, y)) => y - 11,
        };
        let mut first = true;
        for i in 0..5 {
            if self.mem.u8(line(i)) == 1 {
                let y = if first { base_y } else { self.mem.i16(line(i) - LINE_SIZE + 4) - 10 };
                self.mem.set16(line(i) + 4, y);
                first = false;
            }
        }
        for i in (0..5).rev() {
            let l = line(i);
            if self.mem.u8(l) == 1 && self.mem.i16(l + 4) < 5 {
                let mut push = 0;
                while self.mem.i16(l + 4) < 5 {
                    self.mem.set16(l + 4, self.mem.i16(l + 4) + 1);
                    push += 1;
                }
                for k in (0..i).rev() {
                    self.mem.set16(line(k) + 4, self.mem.i16(line(k) + 4) + push);
                }
                break;
            }
        }
        let ts = self.mem.u8(g::TEXT_TICKS);
        self.mem.set16(SPEECH_TIMER, if chars >= 10 { ts * chars } else { ts * (chars + 2) });
    }

    // ------------------------------------------------------------ label

    /// 0x4d56: build the label line (held object, hovered object, hovered thing).
    fn build_label(&mut self) {
        let mut s: Vec<u8> = Vec::new();
        let mut w = 0;
        let adv = |e: &Engine, c: u8| e.font.size(c).0 - 1;
        if self.mem.u8(held::STATE) != 0 {
            let name = self.mem.cstr(slot(self.mem.u8(held::SLOT) as usize), 30);
            for &c in &name {
                w += adv(self, c);
            }
            s.extend(&name);
            // QUIRK: " & " uses the full glyph widths
            for c in [b' ', b'&', b' '] {
                w += self.font.size(c).0;
            }
            s.extend(b" & ");
        }
        let h = self.mem.i16(HOVER_INV);
        if h != 0 && self.mem.u8(hover_rec(h) + 0x1e) != 0 {
            let st = self.mem.u8(held::STATE);
            if (st == 1 && self.mem.u8(hover_rec(h) + 0x20) != self.mem.u8(held::OBJ)) || st == 0 {
                let name = self.mem.cstr(hover_rec(h), 30);
                for &c in &name {
                    w += adv(self, c);
                }
                s.extend(&name);
                let o = self.mem.u8(hover_rec(h) + 0x20);
                self.mem.set8(held::HOVER_OBJ, o);
            }
        }
        let t = self.mem.u8(HOVER_THING);
        if t != 0 {
            let name = self.mem.cstr(thing(t as usize - 1) + 0x26, 15);
            for &c in &name {
                w += adv(self, c);
            }
            s.extend(&name);
            self.mem.set8(label::THING, t);
        }
        if s.is_empty() && self.mem.u8(held::STATE) == 0 {
            // nothing written: the old string stays
        } else {
            let mut z = s.clone();
            z.push(0);
            self.mem.write(label::TEXT, &z[..z.len().min(0x50)]);
        }
        if w > 0 {
            self.mem.set16(label::W, w);
            self.mem.set16(label::H, 10);
            self.mem.set16(label::X, 160 - w / 2);
            self.mem.set16(label::Y, 0x9b);
            self.mem.set8(label::STATE, 1);
            self.mem.set8(label::DRAWN, 0xff);
        }
    }

    // ------------------------------------------------------------ drawing

    /// 0x5f4 / 0x705: transparent blit, optionally mirrored, clipped to
    /// rows 0..=`maxy`. Sizes are 16-bit counters (0 means 65536).
    #[allow(clippy::too_many_arguments)]
    fn blit(dst: &mut [u8], src: &[u8], sx: i32, sy: i32, dx: i32, dy: i32, w: i32, h: i32, mirror: bool, maxy: i32) {
        let cols = if w & 0xffff == 0 { 65536 } else { w & 0xffff };
        let rows = if h & 0xffff == 0 { 65536 } else { h & 0xffff };
        for r in 0..rows {
            let (yy, syy) = (dy + r, sy + r);
            if yy > maxy || syy > 199 {
                break;
            }
            if yy < 0 || syy < 0 {
                continue;
            }
            for c in 0..cols {
                let xx = dx + c;
                let sxx = if mirror { sx + w - 1 - c } else { sx + c };
                if xx > 319 || (!mirror && sxx > 319) || (mirror && sxx < 0) {
                    break;
                }
                if xx < 0 || !(0..320).contains(&sxx) {
                    continue;
                }
                let v = src[(syy * 320 + sxx) as usize];
                if v != 0 {
                    dst[(yy * 320 + xx) as usize] = v;
                }
            }
        }
    }

    fn draw_thing(&mut self, t: usize) {
        let m = &self.mem;
        let a = thing(t);
        Self::blit(
            &mut self.overlay,
            &self.sheets[1],
            m.i16(a),
            m.i16(a + 2),
            m.i16(a + 0x10),
            m.i16(a + 0x12),
            m.i16(a + 0x18),
            m.i16(a + 0x1a),
            m.u8(a + 0x24) == 1,
            0x94,
        );
    }

    fn draw_player(&mut self) {
        let m = &self.mem;
        Self::blit(
            &mut self.overlay,
            &self.sheets[2],
            m.i16(p::SRCX),
            m.i16(p::SRCY),
            m.i16(p::PX),
            m.i16(p::PY),
            m.i16(p::PW),
            m.i16(p::PH),
            m.u8(p::FLIP) == 1,
            0x94,
        );
    }

    fn draw_lines(&mut self) {
        let border = self.mem.u8(g::SPEECH_BORDER) as u8;
        for i in 0..5 {
            let l = LINES + LINE_SIZE * i;
            if self.mem.u8(l) == 1 {
                let text = self.mem.cstr(l + 0x12, 0x39);
                let (x, y, c) = (self.mem.i16(l + 2), self.mem.i16(l + 4), self.mem.u8(l + 0x65) as u8);
                self.font.text(&mut self.overlay, x, y, &text, c, border);
            }
        }
    }

    fn draw_label(&mut self) {
        let m = &self.mem;
        let text = m.cstr(label::TEXT, 0x50);
        let (x, y) = (m.i16(label::X), m.i16(label::Y));
        let (c, b) = (m.u8(label::COLOUR) as u8, m.u8(label::BORDER) as u8);
        self.font.text(&mut self.overlay, x, y, &text, c, b);
    }

    fn draw_icon(&mut self, i: usize) {
        let o = self.mem.u8(slot(self.mem.u8(g::INV_SCROLL) as usize + i) + 0x20);
        Self::blit(
            &mut self.overlay,
            &self.sheets[3],
            (o % 9) * 35,
            (o / 9) * 26,
            13 + 37 * i as i32,
            173,
            35,
            26,
            false,
            199,
        );
    }

    /// 0x39d0 / 0x3a7c: clear columns x..=x+w, rows y..y+h-1 of the overlay.
    fn clear(&mut self, x: i32, y: i32, w: i32, h: i32, maxy: i32) {
        if x + w < 0 || y + h < 0 || x > 319 || y > 199 {
            return;
        }
        for yy in y.max(0)..(y + h).min(maxy + 1) {
            for xx in x.max(0)..=(x + w).min(319) {
                self.overlay[(yy * 320 + xx) as usize] = 0;
            }
        }
    }

    /// 0x3cb5 (`only_overlay` = false) / 0x3b28: copy a rectangle to the screen.
    fn present_rect(&mut self, x: i32, y: i32, w: i32, h: i32, only_overlay: bool) {
        for yy in y.max(0)..(y + h).min(200) {
            for xx in x.max(0)..=(x + w).min(319) {
                let i = (yy * 320 + xx) as usize;
                let o = self.overlay[i];
                if o != 0 {
                    self.vga[i] = o;
                } else if !only_overlay {
                    self.vga[i] = self.sheets[0][i];
                }
            }
        }
    }

    /// The whole screen = overlay over background.
    pub(super) fn present_all(&mut self) {
        for i in 0..64000 {
            let o = self.overlay[i];
            self.vga[i] = if o != 0 { o } else { self.sheets[0][i] };
        }
    }

    fn thing_visible(&self, t: usize) -> bool {
        let st = self.mem.i16(thing(t) + 0x22);
        st != 0 && st != 3
    }

    fn draw_scene(&mut self, things: usize) {
        for t in 0..things {
            let layer = self.mem.i16(thing(t) + 0x0c);
            if self.thing_visible(t) && (layer == 0 || layer == 2) {
                self.draw_thing(t);
            }
        }
        if self.mem.i16(p::VISIBLE) != 0 {
            self.draw_player();
        }
        for t in 0..things {
            let layer = self.mem.i16(thing(t) + 0x0c);
            if self.thing_visible(t) && (layer == 1 || layer == 3) {
                self.draw_thing(t);
            }
        }
    }

    /// 0x61df: redraw the overlay from scratch (things 0..24 only, quirk).
    pub(super) fn full_redraw(&mut self) {
        self.overlay.fill(0);
        self.draw_scene(25);
        self.draw_lines();
        if self.mem.u8(label::STATE) != 0 {
            self.draw_label();
        }
        for i in 0..8 {
            if self.mem.u8(slot(self.mem.u8(g::INV_SCROLL) as usize + i) + 0x1e) != 0 {
                self.draw_icon(i);
            }
        }
    }

    fn thing_changed(&self, t: usize) -> bool {
        let a = thing(t);
        let m = &self.mem;
        [(0x22, 0x20), (0, 4), (2, 6), (0x10, 0x14), (0x12, 0x16), (0x18, 0x1c), (0x1a, 0x1e)]
            .iter()
            .any(|&(c, o)| m.i16(a + c) != m.i16(a + o))
    }

    fn player_changed(&self) -> bool {
        let m = &self.mem;
        m.i16(p::PX) != m.i16(0x91e1)
            || m.i16(p::PY) != m.i16(0x91e3)
            || m.i16(p::PW) != m.i16(0x91ed)
            || m.i16(p::PH) != m.i16(0x91ef)
            || m.u8(p::FLIP) != m.u8(0x91f8)
            || m.u8(p::FRAME) != m.u8(p::LAST_FRAME)
            || m.i16(p::VISIBLE) != m.i16(0x91f3)
    }

    /// The player's rectangle to erase/present: the old one, or the current
    /// position with the frame size while speaking.
    fn player_old_rect(&self) -> (i32, i32, i32, i32) {
        let m = &self.mem;
        if m.u8(p::SPEAKING) != 0 {
            (m.i16(p::PX), m.i16(p::PY), m.i16(p::FRAME_W), m.i16(p::FRAME_H))
        } else {
            (m.i16(0x91e1), m.i16(0x91e3), m.i16(0x91ed), m.i16(0x91ef))
        }
    }

    fn slot_changed(&self, i: usize) -> bool {
        let r = slot(self.mem.u8(g::INV_SCROLL) as usize + i);
        self.mem.u8(r + 0x1e) != self.mem.u8(r + 0x1f) || self.mem.u8(r + 0x20) != self.mem.u8(r + 0x21)
    }

    /// One main-loop pass worth of drawing: erase changed areas, redraw the
    /// overlay, wait (or load the palette), and copy the changes to the screen.
    fn compose_pass(&mut self) {
        self.mem.set8(COMPOSING, 1);
        let changed: Vec<bool> = (0..30).map(|t| self.thing_changed(t)).collect();
        let player = self.player_changed();
        let lines_off: Vec<bool> = (0..5)
            .map(|i| {
                let l = LINES + LINE_SIZE * i;
                self.mem.u8(l) == 0 && self.mem.u8(l + 1) != 0
            })
            .collect();
        let lines_on: Vec<bool> = (0..5)
            .map(|i| {
                let l = LINES + LINE_SIZE * i;
                self.mem.u8(l) != 0 && self.mem.u8(l + 1) == 0
            })
            .collect();
        let label_changed = self.mem.u8(label::STATE) != self.mem.u8(label::DRAWN);
        let slots: Vec<bool> = (0..8).map(|i| self.slot_changed(i)).collect();
        // A: erase
        for t in (0..30).filter(|&t| changed[t]) {
            let a = thing(t);
            let (x, y, w, h) =
                (self.mem.i16(a + 0x14), self.mem.i16(a + 0x16), self.mem.i16(a + 0x1c), self.mem.i16(a + 0x1e));
            self.clear(x, y, w, h, 199);
        }
        for i in (0..5).filter(|&i| lines_off[i]) {
            let l = LINES + LINE_SIZE * i;
            let (x, y, w, h) = (self.mem.i16(l + 2), self.mem.i16(l + 4), self.mem.i16(l + 0xa), self.mem.i16(l + 0xc));
            self.clear(x, y, w, h, 199);
        }
        if player {
            let (x, y, w, h) = self.player_old_rect();
            self.clear(x, y, w, h, 148);
        }
        if label_changed {
            let m = &self.mem;
            let (x, y, w, h) = (m.i16(label::OLD_X), m.i16(label::OLD_Y), m.i16(label::OLD_W), m.i16(label::OLD_H));
            self.clear(x, y, w, h, 199);
        }
        for i in (0..8).filter(|&i| slots[i]) {
            self.clear(13 + 37 * i as i32, 173, 35, 26, 199);
        }
        // B: draw
        self.draw_scene(30);
        self.draw_lines();
        if label_changed && self.mem.u8(label::STATE) == 1 {
            self.draw_label();
        }
        for (i, _) in slots.iter().enumerate().filter(|s| *s.1) {
            if self.mem.u8(slot(self.mem.u8(g::INV_SCROLL) as usize + i) + 0x1e) != 0 {
                self.draw_icon(i);
            }
        }
        self.mem.set8(COMPOSING, 0);
        if self.mem.u8(g::PAL_DIRTY) == 1 {
            let pal = self.mem.slice(g::PAL_SCENE, 768);
            self.set_dac(&pal);
            self.mem.set8(g::PAL_DIRTY, 0);
        }
        self.wait(2.0 * RETRACE_MS);
        // C: present
        if player {
            let (x, y, w, h) = self.player_old_rect();
            self.present_rect(x, y, w, h, false);
        }
        for t in (0..30).filter(|&t| changed[t]) {
            let a = thing(t);
            let (x, y, w, h) =
                (self.mem.i16(a + 0x14), self.mem.i16(a + 0x16), self.mem.i16(a + 0x1c), self.mem.i16(a + 0x1e));
            self.present_rect(x, y, w, h, false);
        }
        for i in (0..5).filter(|&i| lines_off[i]) {
            let l = LINES + LINE_SIZE * i;
            let (x, y, w, h) = (self.mem.i16(l + 2), self.mem.i16(l + 4), self.mem.i16(l + 0xa), self.mem.i16(l + 0xc));
            self.present_rect(x, y, w, h, false);
        }
        if label_changed {
            let m = &self.mem;
            let (x, y, w, h) = (m.i16(label::OLD_X), m.i16(label::OLD_Y), m.i16(label::OLD_W), m.i16(label::OLD_H));
            self.present_rect(x, y, w, h, false);
        }
        for i in (0..8).filter(|&i| slots[i]) {
            self.present_rect(13 + 37 * i as i32, 173, 35, 26, false);
        }
        for t in (0..30).filter(|&t| changed[t]) {
            let a = thing(t);
            if self.thing_visible(t) {
                let (x, y, w, h) =
                    (self.mem.i16(a + 0x10), self.mem.i16(a + 0x12), self.mem.i16(a + 0x18), self.mem.i16(a + 0x1a));
                self.present_rect(x, y, w, h, true);
            }
            for (c, o) in [(0x22, 0x20), (0, 4), (2, 6), (0x10, 0x14), (0x12, 0x16), (0x18, 0x1c), (0x1a, 0x1e)] {
                let v = self.mem.i16(a + c);
                self.mem.set16(a + o, v);
            }
        }
        if player {
            let m = &self.mem;
            if m.i16(p::VISIBLE) == 1 {
                let (x, y) = (m.i16(p::PX), m.i16(p::PY));
                let (w, h) = if m.u8(p::SPEAKING) != 0 {
                    (m.i16(p::FRAME_W), m.i16(p::FRAME_H))
                } else {
                    (m.i16(p::PW), m.i16(p::PH))
                };
                self.present_rect(x, y, w, h, true);
            }
            let m = &mut self.mem;
            for (c, o) in [(p::PX, 0x91e1), (p::PY, 0x91e3), (p::PW, 0x91ed), (p::PH, 0x91ef), (p::VISIBLE, 0x91f3)] {
                let v = m.i16(c);
                m.set16(o, v);
            }
            m.set8(0x91f8, m.u8(p::FLIP));
            m.set8(p::LAST_FRAME, m.u8(p::FRAME));
        }
        for (i, &on) in lines_on.iter().enumerate() {
            let l = LINES + LINE_SIZE * i;
            if on {
                let (x, y, w, h) =
                    (self.mem.i16(l + 2), self.mem.i16(l + 4), self.mem.i16(l + 0xa), self.mem.i16(l + 0xc));
                self.present_rect(x, y, w, h, true);
            }
            let v = self.mem.u8(l);
            self.mem.set8(l + 1, v);
        }
        if label_changed {
            let m = &mut self.mem;
            if m.u8(label::STATE) == 1 {
                let (x, y, w, h) = (m.i16(label::X), m.i16(label::Y), m.i16(label::W), m.i16(label::H));
                self.present_rect(x, y, w, h, true);
            }
            let m = &mut self.mem;
            for (c, o) in
                [(label::X, label::OLD_X), (label::Y, label::OLD_Y), (label::W, label::OLD_W), (label::H, label::OLD_H)]
            {
                let v = m.i16(c);
                m.set16(o, v);
            }
            m.set8(label::DRAWN, m.u8(label::STATE));
        }
        for i in (0..8).filter(|&i| slots[i]) {
            self.present_rect(13 + 37 * i as i32, 173, 35, 26, true);
            let r = slot(self.mem.u8(g::INV_SCROLL) as usize + i);
            let (u, o) = (self.mem.u8(r + 0x1e), self.mem.u8(r + 0x20));
            self.mem.set8(r + 0x1f, u);
            self.mem.set8(r + 0x21, o);
        }
    }
}

pub const HOVER_THING: usize = 0x9202;
