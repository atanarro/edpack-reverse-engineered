//! The CARGAR / GUARDAR / SALIR / JUGAR menu (RUNDAT.EXE 0x9ae1) with its
//! load and save dialogs (0x8d2e / 0x840f), drawn straight onto the screen
//! like the original.  The rest of the game is frozen while it runs.

use crate::engine::{Engine, Key, PASS_MS};
use crate::ui::mouse as mo;

const BORDER: usize = 0xfbe1;
const BG: usize = 0xfbe3;
const LIGHT: usize = 0xfbe5;
const SEL: usize = 0xfbe7;
const TEXT: usize = 0xfbe9;
const TITLE: usize = 0xfbeb;
const TITLE_B: usize = 0xfbed;
const DARK_N: usize = 0xfbdf;
const EMPTY: &[u8] = b"- VACIO -";
const BUTTONS: [(i32, i32, i32, &[u8]); 4] =
    [(46, 60, 49, b"CARGAR"), (61, 74, 63, b"GUARDAR"), (75, 88, 77, b"SALIR"), (89, 102, 91, b"JUGAR")];

struct Painter<'a> {
    e: &'a mut Engine,
    /// the in-game verb, restored while saving so the savegame holds the game cursor
    saved_verb: i32,
}

impl Painter<'_> {
    fn col(&self, a: usize) -> u8 {
        self.e.mem.u8(a) as u8
    }
    fn px(&mut self, x: i32, y: i32, c: u8) {
        if (0..320).contains(&x) && (0..200).contains(&y) {
            self.e.vga[(y * 320 + x) as usize] = c;
        }
    }
    fn fill(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, c: u8) {
        for y in y1..y2 {
            for x in x1..x2 {
                self.px(x, y, c);
            }
        }
    }
    fn rect(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, c: u8) {
        for x in x1..x2 {
            self.px(x, y1, c);
        }
        for y in y1..y2 {
            self.px(x1, y, c);
            self.px(x2, y, c);
        }
        for x in x1..=x2 {
            self.px(x, y2, c);
        }
    }
    fn hline(&mut self, x1: i32, x2: i32, y: i32, c: u8) {
        for x in x1..=x2 {
            self.px(x, y, c);
        }
    }
    fn vline(&mut self, x: i32, y1: i32, y2: i32, c: u8) {
        for y in y1..=y2 {
            self.px(x, y, c);
        }
    }
    fn text(&mut self, x: i32, y: i32, s: &[u8], c: u8, b: u8) {
        let e = &mut *self.e;
        e.font.text(&mut e.vga, x, y, s, c, b);
    }
    fn frame(&mut self, scene: &[u8]) {
        self.e.vga.copy_from_slice(scene);
        let (bg, light, border) = (self.col(BG), self.col(LIGHT), self.col(BORDER));
        self.fill(48, 10, 270, 132, bg);
        let title = self.e.title.clone();
        let w = self.e.font.width(&title);
        let (tc, tb) = (self.col(TITLE), self.col(TITLE_B));
        self.text(160 - w / 2 - 2, 13, &title, tc, tb);
        self.rect(48, 10, 270, 132, light);
        self.rect(49, 11, 269, 131, light);
        self.hline(50, 269, 131, border);
        self.hline(49, 269, 132, border);
        self.vline(269, 11, 131, border);
        self.vline(270, 10, 132, border);
        self.rect(51, 24, 205, 129, border);
        self.hline(51, 205, 129, light);
        self.vline(205, 24, 129, light);
    }
    fn button(&mut self, i: usize, label: &[u8], colour: Option<u8>) {
        let (y1, y2, ly, _) = BUTTONS[i];
        let (light, border, bg, text) = (self.col(LIGHT), self.col(BORDER), self.col(BG), self.col(TEXT));
        self.rect(207, y1, 267, y2, light);
        self.hline(207, 267, y2, border);
        self.vline(267, y1, y2, border);
        let w = self.e.font.width(label);
        self.text(237 - w / 2, ly, label, colour.unwrap_or(text), bg);
    }
    fn slot_row(&mut self, i: i32, s: &[u8], c: u8) {
        let bg = self.col(BG);
        self.fill(52, 25 + 13 * i, 205, 37 + 13 * i, bg);
        self.text(52, 26 + 13 * i, s, c, bg);
    }
    fn hit(&self, y1: i32, y2: i32) -> bool {
        let (hx, hy) = self.e.hotspot();
        (207..=267).contains(&hx) && hy >= y1 && hy <= y2
    }
    fn click(&mut self) -> bool {
        let l = self.e.button(0);
        let r = self.e.button(1);
        l || r
    }
    fn key(&self) -> i32 {
        self.e.key()
    }
    fn slot_under(&self, prev: i32) -> i32 {
        let (hx, hy) = self.e.hotspot();
        (0..8).find(|&i| (51..=205).contains(&hx) && hy >= 25 + 13 * i && hy <= 37 + 13 * i).unwrap_or(prev)
    }
    fn names(&self) -> Vec<Option<Vec<u8>>> {
        (0..8).map(|i| self.e.save_name(i)).collect()
    }
    fn darken(&mut self) {
        let n = self.e.mem.i16(DARK_N);
        let mut pal = self.e.mem.slice(0x2580, 768);
        if n > 0 && n < 255 {
            for (k, v) in pal.iter_mut().enumerate().take(3 * n as usize) {
                *v = (self.e.mem.u8(0x2280 + k) - 20).max(0) as u8;
            }
        }
        self.e.set_dac(&pal);
    }

    /// SAVE (0x9435) first restores the game's cursor, so loading resumes with it.
    fn save(&mut self, slot: usize, name: &[u8]) {
        self.e.mouse_action(self.saved_verb);
        self.e.save_state(slot, name);
        let c = self.e.menu_cursor();
        self.e.mouse_action(c);
    }

    fn load_dialog(&mut self, scene: &[u8]) -> bool {
        self.frame(scene);
        self.button(2, b"ANULAR", None);
        let list = self.names();
        let text = self.col(TEXT);
        for (i, n) in list.iter().enumerate() {
            self.slot_row(i as i32, n.as_deref().unwrap_or(EMPTY), text);
        }
        self.e.mem.set8(0xdc, 0);
        loop {
            if self.key() == 1 {
                self.e.mem.set8(0xdc, 0);
                return false;
            }
            if self.click() {
                if self.hit(75, 88) {
                    let sel = self.col(SEL);
                    self.button(2, b"ANULAR", Some(sel));
                    self.e.wait(PASS_MS);
                    return false;
                }
                let s = self.slot_under(100);
                if s != 100
                    && let Some(name) = &list[s as usize]
                {
                    let sel = self.col(SEL);
                    self.slot_row(s, name, sel);
                    self.e.wait(PASS_MS);
                    return self.e.load_state(s as usize);
                }
            }
            self.e.wait(PASS_MS);
        }
    }

    fn save_dialog(&mut self, scene: &[u8]) -> bool {
        self.frame(scene);
        self.button(1, b"OK", None);
        self.button(2, b"ANULAR", None);
        let list = self.names();
        let (text, selc) = (self.col(TEXT), self.col(SEL));
        for (i, n) in list.iter().enumerate() {
            self.slot_row(i as i32, n.as_deref().unwrap_or(EMPTY), text);
        }
        self.e.mem.set8(0xdc, 0);
        let mut sel = 100;
        while sel == 100 {
            if self.key() == 1 {
                self.e.mem.set8(0xdc, 0);
                return false;
            }
            if self.click() {
                if self.hit(75, 88) {
                    self.button(2, b"ANULAR", Some(selc));
                    self.e.wait(PASS_MS);
                    return false;
                }
                sel = self.slot_under(100);
            }
            if sel == 100 {
                self.e.wait(PASS_MS);
            }
        }
        // text entry
        let mut buf = list[sel as usize].clone().unwrap_or_default();
        self.e.last_char = None;
        loop {
            let mut shown = buf.clone();
            shown.push(b'_');
            self.slot_row(sel, &shown, selc);
            self.e.wait(PASS_MS);
            if self.click() {
                if self.hit(61, 74) && !buf.is_empty() {
                    self.button(1, b"OK", Some(selc));
                    self.save(sel as usize, &buf);
                    return true;
                }
                if self.hit(75, 88) {
                    return false;
                }
                let s = self.slot_under(sel);
                if s != sel {
                    self.slot_row(sel, list[sel as usize].as_deref().unwrap_or(EMPTY), text);
                    sel = s;
                    buf = list[sel as usize].clone().unwrap_or_default();
                }
            }
            match self.e.last_char.take() {
                Some(Key::Enter) if !buf.is_empty() => {
                    self.save(sel as usize, &buf);
                    return true;
                }
                Some(Key::Escape) => return false,
                Some(Key::Backspace) => {
                    buf.pop();
                }
                Some(Key::Char(ch)) => {
                    if let Some(code) = crate::char_to_cp850(ch)
                        && (self.e.font.size(code).0 > 0 || ch == ' ')
                        && self.e.font.width(&buf) < 138
                        && buf.len() < 54
                    {
                        buf.push(code);
                    }
                }
                _ => {}
            }
        }
    }
}

/// MENU(): runs until the player resumes, loads or quits (sets `exited`).
pub fn run_menu(e: &mut Engine) {
    let saved_verb = e.mem.u8(mo::VERB);
    let saved_mode = e.mem.i16(mo::MODE);
    let c = e.menu_cursor();
    e.mouse_action(c);
    let scene = e.vga.clone();
    let mut pt = Painter { e, saved_verb };
    let mut done = false;
    while !done {
        pt.darken();
        pt.frame(&scene);
        for (i, b) in BUTTONS.iter().enumerate() {
            pt.button(i, b.3, None);
        }
        pt.e.mem.set16(mo::MODE, 0);
        pt.e.mem.set8(0xdc, 0);
        let mut redraw = false;
        while !redraw && !done {
            pt.e.wait(PASS_MS);
            if pt.key() == 1 {
                pt.e.mem.set8(0xdc, 0);
                done = true;
                break;
            }
            if !pt.click() {
                continue;
            }
            let selc = pt.col(SEL);
            if pt.hit(89, 101) {
                pt.button(3, b"JUGAR", Some(selc));
                done = true;
            } else if pt.hit(75, 88) {
                pt.button(2, b"SALIR", Some(selc));
                pt.fill(0, 93, 320, 108, 0);
                pt.rect(0, 93, 320, 108, 255);
                pt.text(125, 96, b"\xa8Salir (S/N)?", 255, 0);
                pt.e.mem.set8(0xdc, 0);
                while pt.key() == 0 {
                    pt.e.wait(PASS_MS);
                }
                if pt.key() == 0x1f {
                    pt.e.exited = true;
                    return;
                }
                redraw = true;
            } else if pt.hit(61, 73) {
                pt.button(1, b"GUARDAR", Some(selc));
                if pt.save_dialog(&scene) { done = true } else { redraw = true }
            } else if pt.hit(46, 60) {
                pt.button(0, b"CARGAR", Some(selc));
                if pt.load_dialog(&scene) {
                    let e = pt.e;
                    e.mem.set16(mo::MODE, saved_mode);
                    let cur = e.mem.slice(0x2580, 768);
                    e.set_dac(&cur);
                    return;
                }
                redraw = true;
            }
        }
    }
    let e = pt.e;
    e.mouse_action(saved_verb);
    e.mem.set16(mo::MODE, saved_mode);
    e.compose();
    let cur = e.mem.slice(0x2580, 768);
    e.set_dac(&cur);
    e.mem.set8(0xdc, 0);
}
