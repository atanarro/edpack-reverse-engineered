//! The SPACE menu (0xabda) and the save/load dialogs (0x8046 / 0x915b).
//! See docs/v1-runvga/03-mainloop-ui.md §9.

use crate::host::{Exit, Key};

use super::engine::{Engine, RETRACE_MS, g};

const PANEL: u8 = 251;
const LIGHT: u8 = 252;
const DARK: u8 = 250;
const TEXT: u8 = 0xfe;
const TEXT_BORDER: u8 = 0xfb;
const PICKED: u8 = 0xfd;
const SLOTS: usize = 8;
const SAVE_MAGIC: &[u8] = b"EDV1SAVE1\0";

enum Choice {
    Resume,
    Menu,
}

impl Engine {
    fn fill(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, c: u8) {
        for y in y1.max(0)..y2.min(200) {
            for x in x1.max(0)..x2.min(320) {
                self.overlay[(y * 320 + x) as usize] = c;
            }
        }
    }

    fn hline(&mut self, x1: i32, x2: i32, y: i32, c: u8) {
        if (0..200).contains(&y) {
            for x in x1.max(0)..=x2.min(319) {
                self.overlay[(y * 320 + x) as usize] = c;
            }
        }
    }

    fn vline(&mut self, x: i32, y1: i32, y2: i32, c: u8) {
        if (0..320).contains(&x) {
            for y in y1.max(0)..=y2.min(199) {
                self.overlay[(y * 320 + x) as usize] = c;
            }
        }
    }

    fn outline(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, c: u8) {
        self.hline(x1, x2, y1, c);
        self.hline(x1, x2, y2, c);
        self.vline(x1, y1, y2, c);
        self.vline(x2, y1, y2, c);
    }

    fn button_frame(&mut self, y1: i32, y2: i32) {
        self.outline(214, y1, 260, y2, LIGHT);
        self.hline(214, 260, y2, DARK);
        self.vline(260, y1, y2, DARK);
    }

    fn write(&mut self, x: i32, y: i32, s: &[u8], c: u8, border: u8) {
        self.font.text(&mut self.overlay, x, y, s, c, border);
    }

    /// The panel and list box shared by the menu and the dialogs.
    fn panel(&mut self) {
        self.fill(48, 20, 270, 130, PANEL);
        self.outline(48, 20, 270, 130, LIGHT);
        self.outline(49, 21, 269, 129, LIGHT);
        self.hline(50, 269, 129, DARK);
        self.hline(49, 269, 130, DARK);
        self.vline(269, 21, 129, DARK);
        self.vline(270, 20, 130, DARK);
        self.outline(51, 24, 205, 127, DARK);
        self.hline(51, 205, 127, LIGHT);
        self.vline(205, 24, 127, LIGHT);
    }

    /// The cursor's top-left (the menus ignore the hotspot).
    fn cursor(&self) -> (i32, i32) {
        (self.mem.i16(g::MOUSE_X), self.mem.i16(g::MOUSE_Y))
    }

    /// Wait for a click (either button) or a key; returns the key scan code if any.
    fn menu_event(&mut self) -> Option<u8> {
        self.mem.set8(g::KEY, 0);
        self.last_char = None;
        loop {
            self.wait(2.0 * RETRACE_MS);
            if self.button(0) || self.button(1) {
                return None;
            }
            let k = self.mem.u8(g::KEY);
            if k != 0 {
                self.mem.set8(g::KEY, 0);
                return Some(k as u8);
            }
        }
    }

    fn hide_cursor_and_wait(&mut self) {
        self.mem.set16(g::CURSOR_STATE, 2);
        while self.mem.i16(g::CURSOR_STATE) != 3 {
            self.wait(RETRACE_MS);
        }
    }

    /// SPACE: Cargar / Guardar / Salir / Jugar.
    pub(super) fn menu(&mut self) {
        let (mode, drawn) = (self.mem.i16(g::CURSOR_STATE), self.mem.i16(0x92b2));
        'menu: loop {
            self.hide_cursor_and_wait();
            self.panel();
            for y in [(46, 60), (61, 74), (75, 88), (89, 102)] {
                self.button_frame(y.0, y.1);
            }
            let labels: [(&[u8], i32, i32); 4] =
                [(b"Cargar", 219, 49), (b"Guardar", 216, 63), (b"Salir", 225, 77), (b"Jugar", 221, 91)];
            for (s, x, y) in labels {
                self.write(x, y, s, TEXT, TEXT_BORDER);
            }
            self.present_all();
            self.mem.set16(g::CURSOR_STATE, 0);
            loop {
                if let Some(k) = self.menu_event() {
                    if k == 1 {
                        break 'menu;
                    }
                    continue;
                }
                let (x, y) = self.cursor();
                if !(215..=259).contains(&x) {
                    continue;
                }
                if (89..=101).contains(&y) {
                    self.write(221, 91, b"Jugar", PICKED, TEXT_BORDER);
                    break 'menu;
                }
                if (75..=259).contains(&y) {
                    // QUIRK: the Salir hit box reaches down to y 259
                    self.write(225, 77, b"Salir", PICKED, TEXT_BORDER);
                    if self.confirm_quit() {
                        self.exited = true;
                        std::panic::panic_any(Exit);
                    }
                    self.full_redraw();
                    self.present_all();
                    continue 'menu;
                }
                if (61..=73).contains(&y) {
                    self.write(216, 63, b"Guardar", PICKED, TEXT_BORDER);
                    match self.save_dialog() {
                        Choice::Resume => break 'menu,
                        Choice::Menu => continue 'menu,
                    }
                }
                if (46..=60).contains(&y) {
                    self.write(219, 49, b"Cargar", PICKED, TEXT_BORDER);
                    match self.load_dialog() {
                        Choice::Resume => break 'menu,
                        Choice::Menu => continue 'menu,
                    }
                }
            }
        }
        self.full_redraw();
        self.present_all();
        self.mem.set16(g::CURSOR_STATE, mode);
        self.mem.set16(0x92b2, drawn);
    }

    fn confirm_quit(&mut self) -> bool {
        self.hide_cursor_and_wait();
        self.fill(0, 93, 320, 108, 0);
        self.outline(0, 93, 319, 108, 255);
        let mut text = vec![0xa8]; // ¿ in CP850
        text.extend(b"Salir (S/N)?");
        self.write(125, 96, &text, 255, 0);
        self.present_all();
        loop {
            self.wait(2.0 * RETRACE_MS);
            let k = self.mem.u8(g::KEY);
            if k != 0 {
                self.mem.set8(g::KEY, 0);
                return k == 0x1f;
            }
        }
    }

    // ------------------------------------------------------------ savegames

    fn save_key(&self, slot: usize) -> String {
        format!("{}/SAVEGAME._{}_", self.save_prefix, slot + 1)
    }

    pub(super) fn save_names(&self) -> Vec<Option<Vec<u8>>> {
        (0..SLOTS)
            .map(|s| {
                let raw = self.storage.get(&self.save_key(s))?;
                let rest = raw.strip_prefix(SAVE_MAGIC)?;
                let n = *rest.first()? as usize;
                rest.get(1..1 + n).map(<[u8]>::to_vec)
            })
            .collect()
    }

    pub fn save_state(&mut self, slot: usize, name: &[u8]) {
        let mut out = SAVE_MAGIC.to_vec();
        out.push(name.len().min(49) as u8);
        out.extend(&name[..name.len().min(49)]);
        out.extend(&self.mem.bytes);
        out.extend(self.code.iter().flat_map(|w| w.to_le_bytes()));
        let key = self.save_key(slot);
        self.storage.set(&key, &out);
    }

    pub fn load_state(&mut self, slot: usize) -> bool {
        let Some(raw) = self.storage.get(&self.save_key(slot)) else { return false };
        let Some(rest) = raw.strip_prefix(SAVE_MAGIC) else { return false };
        let n = rest[0] as usize;
        let body = &rest[1 + n..];
        let size = self.mem.bytes.len();
        if body.len() != size + 10000 {
            return false;
        }
        self.mem.bytes.copy_from_slice(&body[..size]);
        self.code = body[size..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        for s in 0..4 {
            self.load_sheet(s, 0x6e);
        }
        let music = self.mem.i16(g::MUSIC);
        self.mixer.lock().unwrap().stop_music();
        if music >= 0
            && let Ok(b) = self.file_bytes(music)
        {
            let _ = self.mixer.lock().unwrap().play_music(&b);
        }
        self.fade_out();
        self.full_redraw();
        self.inventory_arrows();
        self.present_all();
        let pal = self.mem.slice(g::PAL_SCENE, 768);
        self.fade_in(&pal);
        true
    }

    fn slot_list(&mut self, names: &[Option<Vec<u8>>]) {
        for (i, n) in names.iter().enumerate() {
            let y = 25 + 13 * i as i32;
            self.fill(52, y, 205, y + 10, PANEL);
            let s = n.clone().unwrap_or_else(|| b"- VACIO -".to_vec());
            self.write(52, y + 1, &s, TEXT, TEXT_BORDER);
        }
    }

    fn slot_at(&self) -> Option<usize> {
        let (x, y) = self.cursor();
        if !(51..=205).contains(&x) {
            return None;
        }
        (0..SLOTS).find(|&i| (24 + 13 * i as i32..=35 + 13 * i as i32).contains(&y))
    }

    fn dialog(&mut self, ok: bool) -> Vec<Option<Vec<u8>>> {
        self.hide_cursor_and_wait();
        self.panel();
        if ok {
            self.button_frame(61, 74);
            self.write(230, 63, b"Ok", TEXT, TEXT_BORDER);
        }
        self.button_frame(75, 88);
        self.write(220, 77, b"Anular", TEXT, TEXT_BORDER);
        let names = self.save_names();
        self.slot_list(&names);
        self.present_all();
        self.mem.set16(g::CURSOR_STATE, 0);
        names
    }

    fn load_dialog(&mut self) -> Choice {
        let names = self.dialog(false);
        loop {
            if let Some(k) = self.menu_event() {
                if k == 1 {
                    return Choice::Menu;
                }
                continue;
            }
            let (x, y) = self.cursor();
            if (215..=259).contains(&x) && (75..=88).contains(&y) {
                return Choice::Menu;
            }
            if let Some(s) = self.slot_at()
                && names[s].is_some()
            {
                self.hide_cursor_and_wait();
                if self.load_state(s) {
                    return Choice::Resume;
                }
                return Choice::Menu;
            }
        }
    }

    fn save_dialog(&mut self) -> Choice {
        let mut names = self.dialog(true);
        let mut editing: Option<(usize, Vec<u8>)> = None;
        loop {
            if let Some((s, name)) = &mut editing {
                // text entry
                self.wait(2.0 * RETRACE_MS);
                let key = self.last_char.take();
                let scan = self.mem.u8(g::KEY);
                self.mem.set8(g::KEY, 0);
                let mut redraw = false;
                match key {
                    Some(Key::Enter) if !name.is_empty() => {
                        let (s, n) = (*s, name.clone());
                        self.save_state(s, &n);
                        return Choice::Resume;
                    }
                    Some(Key::Escape) => {
                        editing = None;
                        names = self.save_names();
                        self.slot_list(&names);
                        self.present_all();
                        continue;
                    }
                    Some(Key::Backspace) => {
                        name.pop();
                        redraw = true;
                    }
                    Some(Key::Char(c)) => {
                        if let Some(b) = accepted(c)
                            && self.font.width(name) + self.font.advance(b) < 138
                        {
                            name.push(b);
                            redraw = true;
                        }
                    }
                    _ => {
                        if scan == 1 {
                            editing = None;
                            continue;
                        }
                    }
                }
                if self.button(0) || self.button(1) {
                    let (x, y) = self.cursor();
                    if (215..=259).contains(&x) && (61..=74).contains(&y) && !name.is_empty() {
                        let (s, n) = (*s, name.clone());
                        self.save_state(s, &n);
                        return Choice::Resume;
                    }
                    if (215..=259).contains(&x) && (75..=88).contains(&y) {
                        return Choice::Menu;
                    }
                }
                if redraw {
                    let (s, mut shown) = (*s, name.clone());
                    shown.push(b'_');
                    let y = 25 + 13 * s as i32;
                    self.fill(52, y, 205, y + 10, PANEL);
                    self.write(52, y + 1, &shown, TEXT, TEXT_BORDER);
                    self.present_all();
                }
                continue;
            }
            if let Some(k) = self.menu_event() {
                if k == 1 {
                    return Choice::Menu;
                }
                continue;
            }
            let (x, y) = self.cursor();
            if (215..=259).contains(&x) && (75..=88).contains(&y) {
                return Choice::Menu;
            }
            if let Some(s) = self.slot_at() {
                let name = names[s].clone().unwrap_or_default();
                let mut shown = name.clone();
                shown.push(b'_');
                let y = 25 + 13 * s as i32;
                self.fill(52, y, 205, y + 10, PANEL);
                self.write(52, y + 1, &shown, TEXT, TEXT_BORDER);
                self.present_all();
                self.last_char = None;
                editing = Some((s, name));
            }
        }
    }
}

/// Characters the save dialog accepts, as CP850.
fn accepted(c: char) -> Option<u8> {
    if c.is_ascii_alphanumeric() || " .,/-;:".contains(c) {
        return Some(c as u8);
    }
    let b = crate::char_to_cp850(c)?;
    [0xa0, 0x82, 0xa1, 0xa2, 0xa3, 0x85, 0x8a, 0x8d, 0x95, 0x97, 0x84, 0x89, 0x8b, 0x94, 0x81, 0xa4, 0xa5]
        .contains(&b)
        .then_some(b)
}
