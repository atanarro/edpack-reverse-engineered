//! Speech text, information bar, dialogue choices (STEXT) and the inventory
//! panel.  Ports of RUNDAT.EXE 0x726d (layout), 0x769c (text update),
//! 0x52c0/0x54ed (info bar / hover), 0xabf8/0xaefd/0xada8 (STEXT) and
//! 0xb4e3/0xb1a6/0x6a0a (inventory).

use crate::engine::Engine;
use crate::font::LINE_HEIGHT;
use crate::level::text_string;
use crate::memory::{THING_SIZE, ds};
use crate::player as p;
use crate::specials::INV_RECORD;

pub mod text {
    pub const ACTIVE: usize = 0xfca9;
    pub const ID: usize = 0xfcab;
    pub const SEQ: usize = 0xfcc1;
    pub const LAID: usize = 0xfcc3;
    pub const SPEAKER: usize = 0xfcbf;
    pub const PX: usize = 0xfcc5;
    pub const PY: usize = 0xfcc7;
    pub const PCOLOR: usize = 0xfcd8;
    pub const TIMER: usize = 0xfa2e;
    pub const TICKS_PER_CHAR: usize = 0x8a24;
    pub const SLOTS: usize = 0xf9cb;
    pub const SLOT_SIZE: usize = 0x67;
    pub const COLOR: usize = 0xfcd4;
    pub const BORDER: usize = 0xfcd6;
    pub const PBORDER: usize = 0xfcda;
}
pub mod bar {
    pub const STATE: usize = 0x8a29;
    pub const LAST: usize = 0x8a2a;
    pub const X: usize = 0x8a2b;
    pub const Y: usize = 0x8a2d;
    pub const LABEL_THING: usize = 0x8a33;
    pub const W: usize = 0x8a34;
    pub const TEXT: usize = 0x8a3c;
    pub const COLOR: usize = 0x8a25;
    pub const BORDER: usize = 0x8a27;
    pub const ENABLED: usize = 0xfd01;
}
pub mod inv {
    pub const SLOTS: usize = 0x8a95;
    pub const FIRST: usize = 0xfccc;
    pub const OPEN: usize = 0xfd2b;
    pub const DRAWN: usize = 0x9b;
    pub const LAST_HOVER: usize = 0x99;
    pub const HOVER: usize = 0xfca7;
    pub const HELD: usize = 0x8a8c;
    pub const HELD_SLOT: usize = 0x8a8d;
    pub const HELD_OBJ: usize = 0x8a8e;
    pub const SHOWN_OBJ: usize = 0x8a8f;
    pub const PENDING: usize = 0x8a90;
    pub const SECOND: usize = 0x8a91;
    pub const SECOND_OBJ: usize = 0x8a92;
    pub const RESULT: usize = 0x8a93;
}
pub mod mouse {
    pub const VERB: usize = 0xfd2c;
    pub const SAVED_VERB: usize = 0xfd2e;
    pub const CLICK_VERB: usize = 0xfd2f;
    pub const MODE: usize = 0xfd40;
    pub const STATE: usize = 0xfd46;
    pub const X: usize = 0xfd4a;
    pub const Y: usize = 0xfd4c;
    pub const SRCX: usize = 0xfd42;
    pub const SRCY: usize = 0xfd44;
    pub const W: usize = 0xfd52;
    pub const H: usize = 0xfd54;
    pub const HOTX: usize = 0xfd5e;
    pub const HOTY: usize = 0xfd60;
    pub const CLICKX: usize = 0xfd5a;
    pub const CLICKY: usize = 0xfd5c;
    pub const HOVER: usize = 0xfc22;
    pub const PREV_HOVER: usize = 0xfc23;
    pub const TABLE: usize = 0x1fc4;
}
pub mod stext {
    pub const MODE: usize = 0x1bbc;
    pub const DRAWN: usize = 0x1bbd;
    pub const HL: usize = 0x1bbe;
    pub const HIDDEN: usize = 0xfbd8;
    pub const ROWY: usize = 0xfbce;
    pub const ACTIVE: usize = 0xfcf0;
}

use bar as b;
use inv as iv;
use mouse as mo;
use stext as st;
use text as tx;

fn slot_addr(i: usize) -> usize {
    tx::SLOTS + i * tx::SLOT_SIZE
}

// ---------------------------------------------------------------- speech text

/// Text by id: <1000 level pool, 1000..1999 inventory slot text, >=2000 message.
pub fn resolve_text(e: &Engine, id: i32) -> Vec<u8> {
    if id < 1000 {
        return text_string(&e.mem, id).unwrap_or_default();
    }
    if id < 2000 {
        let o = (id - 1000) as usize * INV_RECORD;
        let rec = e.inv_records.get(o..o + 250).unwrap_or(&[]);
        return rec.iter().take_while(|&&c| c != 0).copied().collect();
    }
    let i = (id - 2000) as usize;
    if i >= 5 {
        return Vec::new();
    }
    e.mem.cstr(ds::MESSAGES + i * 250, 250)
}

pub fn clear_texts(e: &mut Engine) {
    for i in 0..5 {
        e.mem.set8(slot_addr(i), 0);
    }
    e.mem.set16(tx::TIMER, 0);
}

/// Layout (0x726d): split into lines, centre, clamp, set the display time.
pub fn layout_text(e: &mut Engine, cx: i32, cy: i32, s: &[u8], colour: i32, border: i32) {
    let s = if s.is_empty() { e.mem.cstr(ds::MESSAGES, 250) } else { s.to_vec() };
    let (sep, m) = (e.font.sep, &mut e.mem);
    for i in 0..5 {
        m.set8(slot_addr(i), 0);
        m.set8(slot_addr(i) + 0x12, 0);
    }
    let (mut n, mut si) = (0, 0usize);
    for slot in (0..5).rev() {
        let a = slot_addr(slot);
        m.set8(a, 1);
        let (mut w, mut di) = (-sep, 0usize);
        m.set16(a + 0x0c, LINE_HEIGHT);
        m.set8(a + 0x65, colour);
        m.set8(a + 0x66, border);
        while si < s.len() && s[si] != b'^' {
            let c = s[si];
            if di < 56 {
                m.set8(a + 0x12 + di, c as i32);
                w += e.font.advance(c);
                m.set8(a + 0x12 + di + 1, 0);
                n += 1;
            }
            si += 1;
            di += 1;
        }
        m.set16(a + 0x0a, w);
        if si < s.len() && s[si] == b'^' {
            si += 1;
        }
        if si >= s.len() {
            break;
        }
    }
    let active = |m: &crate::memory::Memory, i: usize| m.u8(slot_addr(i)) != 0;
    // vertical stacking: slot 0 is the bottom line
    let mut prev_y: Option<i32> = None;
    for i in 0..5 {
        let a = slot_addr(i);
        if !active(m, i) {
            continue;
        }
        let y = prev_y.map_or(cy - 11, |p| p - 11);
        m.set16(a + 4, y);
        prev_y = Some(y);
        let x = cx - m.i16(a + 0x0a) / 2;
        m.set16(a + 2, x);
    }
    // horizontal clamp of the widest line, the others follow
    let mut wide: Option<usize> = None;
    for i in 0..5 {
        if active(m, i) && wide.is_none_or(|w| m.i16(slot_addr(i) + 0x0a) > m.i16(slot_addr(w) + 0x0a)) {
            wide = Some(i);
        }
    }
    if let Some(wd) = wide {
        let wa = slot_addr(wd);
        let mut delta = 0;
        while m.i16(wa + 2) < 5 {
            let v = m.i16(wa + 2) + 1;
            m.set16(wa + 2, v);
            delta += 1;
        }
        while m.i16(wa + 2) + m.i16(wa + 0x0a) > 315 {
            let v = m.i16(wa + 2) - 1;
            m.set16(wa + 2, v);
            delta -= 1;
        }
        for i in 0..5 {
            if i != wd && active(m, i) {
                let v = m.i16(slot_addr(i) + 2) + delta;
                m.set16(slot_addr(i) + 2, v);
            }
        }
    }
    // vertical clamp (quirks kept: tests < 5 but pushes to 2; dx is not reset)
    let mut dx = 0;
    if let Some(top) = (0..5).rev().find(|&i| active(m, i))
        && m.i16(slot_addr(top) + 4) < 5
    {
        while m.i16(slot_addr(top) + 4) < 2 {
            let v = m.i16(slot_addr(top) + 4) + 1;
            m.set16(slot_addr(top) + 4, v);
            dx += 1;
        }
        for i in 0..top {
            if active(m, i) {
                let v = m.i16(slot_addr(i) + 4) + dx;
                m.set16(slot_addr(i) + 4, v);
            }
        }
    }
    let limit = m.i16(p::AREA_H) - 12;
    if let Some(bot) = (0..5).find(|&i| active(m, i))
        && m.i16(slot_addr(bot) + 4) > limit
    {
        while m.i16(slot_addr(bot) + 4) > limit {
            let v = m.i16(slot_addr(bot) + 4) - 1;
            m.set16(slot_addr(bot) + 4, v);
            dx -= 1;
        }
        for i in bot + 1..5 {
            if active(m, i) {
                let v = m.i16(slot_addr(i) + 4) + dx;
                m.set16(slot_addr(i) + 4, v);
            }
        }
    }
    let t = m.u8(tx::TICKS_PER_CHAR) * if n < 10 { n + 2 } else { n };
    m.set16(tx::TIMER, t);
}

pub fn speak(e: &mut Engine, id: i32) {
    let m = &mut e.mem;
    m.set16(tx::ID, id);
    p::stop(m);
    m.set16(tx::SEQ, 0);
    m.set16(tx::ACTIVE, 1);
    m.set16(tx::SPEAKER, 1);
    let mut f = m.u8(p::FRAME);
    for (start, len) in [(p::S_SIDE, p::N_SIDE), (p::S_BACK, p::N_BACK), (p::S_FRONT, p::N_FRONT)] {
        if f > m.u8(start) && f < m.i8(start) + m.i8(len) - 1 {
            f = m.u8(start);
        }
        m.set8(p::FRAME, f);
    }
    m.set8(p::SAVED_FRAME, f);
    m.set8(p::FRAME, 0);
    m.set16(tx::LAID, -1);
}

pub fn ptext(e: &mut Engine, x: i32, y: i32, id: i32, colour: i32) {
    let m = &mut e.mem;
    m.set16(tx::PX, x);
    m.set16(tx::PY, y);
    m.set16(tx::PCOLOR, colour);
    m.set16(tx::ID, id);
    m.set16(tx::SEQ, 0);
    m.set16(tx::ACTIVE, 1);
    m.set16(tx::SPEAKER, 0);
    m.set16(tx::LAID, -1);
}

/// Per pass while a text is active (0x769c).
pub fn text_frame(e: &mut Engine) {
    if e.mem.i16(tx::SEQ) != e.mem.i16(tx::LAID) {
        let m = &mut e.mem;
        if m.i16(tx::ID) == -1 || m.i16(tx::SEQ) > 0 {
            m.set16(tx::ACTIVE, 0);
            if m.i16(tx::SPEAKER) == 1 {
                m.set16(tx::SPEAKER, 0);
                let f = m.u8(p::SAVED_FRAME);
                m.set8(p::FRAME, f);
                m.set16(p::PREV_VISIBLE, -1);
                let f = f as usize;
                let (sx, sy) = (m.i16(p::POINTS + f * 4), m.i16(p::POINTS + f * 4 + 2));
                m.set16(p::SRCX, sx);
                m.set16(p::SRCY, sy);
                let (w, h) = (m.i16(p::MOV_W), m.i16(p::MOV_H));
                m.set16(p::W, w);
                m.set16(p::H, h);
            }
            return;
        }
        let id = m.i16(tx::ID);
        if m.i16(tx::SPEAKER) == 1 {
            let x = m.i16(p::X) + m.i16(p::W) / 2;
            let y = m.i16(p::Y) + m.i16(p::RED_H);
            let (c, bo) = (m.u8(tx::COLOR), m.u8(tx::BORDER));
            let s = resolve_text(e, id);
            layout_text(e, x, y, &s, c, bo);
        } else {
            let (x, y, c, bo) = (m.i16(tx::PX), m.i16(tx::PY), m.u8(tx::PCOLOR), m.u8(tx::PBORDER));
            let s = if id < 1000 { resolve_text(e, id) } else { Vec::new() };
            layout_text(e, x, y, &s, c, bo);
        }
        let seq = e.mem.i16(tx::SEQ);
        e.mem.set16(tx::LAID, seq);
    }
    if e.mem.i16(tx::TIMER) <= 0 && e.mem.i16(tx::ACTIVE) == 1 {
        clear_texts(e);
        let seq = e.mem.i16(tx::SEQ) + 1;
        e.mem.set16(tx::SEQ, seq);
    }
}

pub fn draw_texts(e: &mut Engine) {
    for i in 0..5 {
        let a = slot_addr(i);
        let m = &e.mem;
        if m.u8(a) == 0 || m.u8(a + 0x12) == 0 {
            continue;
        }
        let s = m.cstr(a + 0x12, 57);
        let (x, y, c, bo) = (m.i16(a + 2), m.i16(a + 4), m.u8(a + 0x65) as u8, m.u8(a + 0x66) as u8);
        e.font.text(&mut e.vga, x, y, &s, c, bo);
    }
}

// ---------------------------------------------------------------- info bar

pub fn inv_slot(e: &Engine, s: usize) -> (bool, i32) {
    let a = iv::SLOTS + s * 6;
    (e.mem.u8(a) != 0, e.mem.i16(a + 2))
}

fn slot_name(e: &Engine, s: usize) -> Vec<u8> {
    let o = s * INV_RECORD + 0xfa;
    e.inv_records.get(o..o + 30).unwrap_or(&[]).iter().take_while(|&&c| c != 0).copied().collect()
}

/// build_label (0x52c0).
pub fn build_label(e: &mut Engine) {
    let mut s = Vec::new();
    if e.mem.u8(iv::HELD) != 0 {
        s.extend(slot_name(e, e.mem.u8(iv::HELD_SLOT) as usize));
        s.extend(b" & ");
    }
    let h = e.mem.u16(iv::HOVER);
    if h != 0 && h <= 30 && inv_slot(e, (h - 1) as usize).0 {
        let (_, obj) = inv_slot(e, (h - 1) as usize);
        let held = e.mem.u8(iv::HELD);
        if held == 0 || (held == 1 && obj != e.mem.u8(iv::HELD_OBJ)) {
            s.extend(slot_name(e, (h - 1) as usize));
            e.mem.set8(iv::SHOWN_OBJ, obj);
            if e.mem.u8(mo::VERB) == 8 && held == 0 {
                s.extend(b" con ");
            }
        }
    }
    let t = e.mem.u8(mo::HOVER);
    if t != 0 {
        s.extend(e.thing_name(t - 1));
        e.mem.set8(b::LABEL_THING, t);
    }
    let w = e.font.width(&s);
    if w > 0 {
        let m = &mut e.mem;
        m.set16(b::W, w);
        m.set16(b::X, 160 - w / 2);
        m.set8(b::STATE, 1);
        m.set8(b::LAST, 0xff);
        // strcpy semantics: the buffer is followed by the inventory state at 8a8c
        let n = s.len().min(0x4f);
        m.write(b::TEXT, &s[..n]);
        m.set8(b::TEXT + n, 0);
    }
}

pub fn draw_info_bar(e: &mut Engine) {
    let m = &e.mem;
    if m.u8(b::ENABLED) != 1 || m.u8(b::STATE) == 0 {
        return;
    }
    let s = m.cstr(b::TEXT, 0x50);
    let (x, y, c, bo) = (m.i16(b::X), m.i16(b::Y), m.u8(b::COLOR) as u8, m.u8(b::BORDER) as u8);
    e.font.text(&mut e.vga, x, y, &s, c, bo);
}

/// hover (0x54ed): first thing in the order list whose AREA contains the hotspot.
pub fn hover(e: &mut Engine) {
    let mode = e.mem.i16(mo::MODE);
    if mode == 1 || mode == 2 {
        return;
    }
    let (px, py) = e.hotspot();
    let m = &mut e.mem;
    for k in 0..30 {
        let t = m.i8(ds::THING_ORDER + k);
        if !(0..30).contains(&t) {
            continue;
        }
        let a = ds::THINGS + t as usize * THING_SIZE;
        let (ax, ay, aw, ah, md) = (m.i16(a), m.i16(a + 2), m.i16(a + 4), m.i16(a + 6), m.u8(a + 0x2b));
        if ax <= px && px <= ax + aw && ay <= py && py <= ay + ah && (md == 1 || md == 3) {
            m.set8(mo::HOVER, t + 1);
            if m.u8(b::LABEL_THING) != t + 1 {
                m.set8(b::STATE, 2);
            }
            return;
        }
    }
    if m.u8(iv::HELD) == 0 && m.u16(iv::HOVER) == 0 {
        m.set8(b::STATE, 0);
    }
    if m.u8(iv::HELD) != 0 && m.u8(b::LABEL_THING) != 0 {
        m.set8(b::STATE, 2);
    }
    m.set8(b::LABEL_THING, 0);
}

// ---------------------------------------------------------------- STEXT

pub fn stext_start(e: &mut Engine, sel: i32, mode: i32, mask: i32) {
    let m = &mut e.mem;
    m.set8(0x1bbf, 0xff);
    m.set8(st::HL, 0xff);
    m.set8(st::DRAWN, 0);
    m.set8(st::MODE, mode);
    m.set8(iv::HELD, 0);
    m.set8(mo::HOVER, 0);
    build_label(e);
    let m = &mut e.mem;
    let mut y = m.i16(0xfd05);
    if y + 57 >= 200 {
        y = 144;
    }
    y = y.max(0);
    m.set16(0xfd05, y);
    m.set16(st::ACTIVE, sel);
    let verb = m.u8(mo::VERB);
    m.set8(mo::SAVED_VERB, verb);
    let base = ds::SELECTEXT + (sel.clamp(0, 49) as usize) * 12;
    let mut digits = mask.abs();
    for i in 0..5 {
        let hidden = digits % 10 != 0 || m.i16(base + i * 2) == -1;
        digits /= 10;
        m.set8(st::HIDDEN + i, hidden as i32);
    }
    let mut k = 0;
    for i in 0..5 {
        if m.u8(st::HIDDEN + i) == 0 {
            m.set16(st::ROWY + i * 2, 11 * k);
            k += 1;
        }
    }
    e.mouse_action(4);
    let x = e.mem.i16(mo::X);
    e.set_mouse(x, y);
}

fn stext_row_text(e: &Engine, sel: i32, i: usize) -> Option<Vec<u8>> {
    let id = e.mem.i16(ds::SELECTEXT + (sel.clamp(0, 49) as usize) * 12 + i * 2);
    if id < 0 { None } else { text_string(&e.mem, id) }
}

/// The dialogue panel, drawn every pass while active (0xada8 + highlight).
pub fn draw_stext(e: &mut Engine) {
    let sel = e.mem.i16(st::ACTIVE);
    if sel < 0 {
        return;
    }
    let big_y = e.mem.i16(0xfd05);
    let (c1, c2) = (e.mem.u8(0xfce8) as u8, e.mem.u8(0xfcea) as u8);
    for r in 0..56 {
        let y = big_y - 1 + r;
        if !(0..200).contains(&y) {
            continue;
        }
        for x in 0..320 {
            let c = if (x + r) & 1 == 0 { c1 } else { c2 };
            if c != 0 {
                e.vga[(y * 320 + x) as usize] = c; // colour 0 is see-through
            }
        }
    }
    let hl = e.mem.u8(st::HL);
    for i in 0..5 {
        let Some(s) = stext_row_text(e, sel, i) else { continue };
        if e.mem.u8(st::HIDDEN + i) != 0 {
            continue;
        }
        let on = i as i32 == hl;
        let m = &e.mem;
        let (c, bo) = (m.u8(if on { 0xfce4 } else { 0xfce0 }) as u8, m.u8(if on { 0xfce6 } else { 0xfce2 }) as u8);
        let y = big_y + m.i16(st::ROWY + i * 2);
        e.font.text_clip(&mut e.vga, 1, y, &s, c, bo);
    }
}

/// Per pass (0xaefd).
pub fn stext_frame(e: &mut Engine) {
    let sel = e.mem.i16(st::ACTIVE);
    if e.mem.u8(st::DRAWN) == 0 {
        if !(0..5).any(|i| stext_row_text(e, sel, i).is_some() && e.mem.u8(st::HIDDEN + i) == 0) {
            e.fatal(0x73, "");
        }
        e.mem.set8(st::DRAWN, 1);
    }
    let (hx, hy) = e.hotspot();
    let big_y = e.mem.i16(0xfd05);
    let base = ds::SELECTEXT + (sel.clamp(0, 49) as usize) * 12;
    let mut row = 0xff;
    for i in 0..5 {
        let ry = big_y + e.mem.i16(st::ROWY + i * 2);
        if (0..320).contains(&hx)
            && ry <= hy
            && hy < ry + 11
            && e.mem.i16(base + i * 2) != -1
            && e.mem.u8(st::HIDDEN + i) == 0
        {
            row = i as i32;
        }
    }
    e.mem.set8(st::HL, row);
    let clicked = {
        let l = e.button(0);
        let r = e.button(1);
        l || r
    };
    if clicked && row <= 4 {
        let mut mask = 0;
        for i in 0..5 {
            if e.mem.u8(st::HIDDEN + i) != 0 || i as i32 == row {
                mask += 10i32.pow(i as u32);
            }
        }
        let m = &mut e.mem;
        m.set16(0xfbdd, mask);
        m.set16(0xfcec, row + 1);
        let id = m.i16(base + row as usize * 2);
        m.set16(0xfcee, id);
        m.set16(st::ACTIVE, -1);
        let verb = m.u8(mo::SAVED_VERB);
        e.mouse_action(verb);
        if e.mem.u8(st::MODE) == 0 {
            speak(e, id);
        }
    }
}

// ---------------------------------------------------------------- inventory

const ARROW_L: [(usize, usize, usize); 9] =
    [(4, 5, 11), (3, 5, 12), (2, 10, 13), (1, 10, 14), (0, 10, 15), (1, 10, 16), (2, 10, 17), (3, 5, 18), (4, 5, 19)];
const ARROW_R: [(usize, usize, usize); 9] = [
    (314, 315, 11),
    (314, 316, 12),
    (309, 317, 13),
    (309, 318, 14),
    (309, 319, 15),
    (309, 318, 16),
    (309, 317, 17),
    (314, 316, 18),
    (314, 315, 19),
];

pub fn draw_inventory(e: &mut Engine) {
    let m = &e.mem;
    let (c1, c2, ca) = (m.u8(0xfd30) as u8, m.u8(0xfd32) as u8, m.u8(0xfd34) as u8);
    for y in 0..30 {
        for x in 0..320 {
            e.vga[y * 320 + x] = if (x + y) & 1 == 0 { c1 } else { c2 };
        }
    }
    let first = m.u8(iv::FIRST) as usize;
    for i in 0..8 {
        let s = first + i;
        if s >= 30 || !inv_slot(e, s).0 {
            continue;
        }
        let rec = &e.inv_records[s * INV_RECORD..(s + 1) * INV_RECORD];
        let (w, h) =
            (rec[0x118] as usize | (rec[0x119] as usize) << 8, rec[0x11a] as usize | (rec[0x11b] as usize) << 8);
        for y in 0..h {
            for x in 0..w {
                let Some(&px) = rec.get(0x11c + y * w + x) else { continue };
                let (dx, dy) = (i * 37 + 13 + x, 2 + y);
                if px != 0 && dx < 320 && dy < 200 {
                    e.vga[dy * 320 + dx] = px;
                }
            }
        }
    }
    let right = first + 8 < 30 && inv_slot(e, first + 8).0;
    let mut span = |list: &[(usize, usize, usize)]| {
        for &(x1, x2, y) in list {
            for x in x1..=x2 {
                e.vga[y * 320 + x] = ca;
            }
        }
    };
    if first > 0 {
        span(&ARROW_L);
    }
    if right {
        span(&ARROW_R);
    }
}

fn hover_slots(e: &mut Engine) {
    let (hx, hy) = e.hotspot();
    let first = e.mem.u8(iv::FIRST) as usize;
    for i in 0..8 {
        let s = first + i;
        if (i * 37 + 13) as i32 <= hx && hx <= (i * 37 + 48) as i32 && hy >= 0 && s < 30 && inv_slot(e, s).0 {
            let m = &mut e.mem;
            m.set16(iv::HOVER, (s + 1) as i32);
            let obj = inv_slot(e, s).1;
            let m = &mut e.mem;
            if m.u8(iv::HELD) != 0 && m.u8(iv::HELD_OBJ) == obj {
                m.set16(iv::HOVER, 0);
                m.set8(iv::SHOWN_OBJ, 0xff);
            }
            let h = m.u16(iv::HOVER);
            let obj_at = if h != 0 { inv_slot(e, (h - 1) as usize).1 } else { e.mem.i16(0x8a91) };
            let m = &mut e.mem;
            if !(m.u8(iv::SHOWN_OBJ) == (obj_at & 0xff) && m.u8(b::STATE) != 0) {
                m.set8(b::STATE, 2);
            }
            return;
        }
    }
    let m = &mut e.mem;
    if m.u8(b::LABEL_THING) == 0 && m.u8(iv::HELD) == 0 {
        m.set8(b::STATE, 0);
    }
    if m.u8(iv::HELD) != 0 && m.u8(iv::SHOWN_OBJ) != 0xff {
        build_label(e);
    }
    e.mem.set8(iv::SHOWN_OBJ, 0xff);
}

/// One pass of the open inventory panel (0xb4e3).
pub fn inventory_frame(e: &mut Engine) {
    e.mem.set8(iv::DRAWN, 1);
    let close = |e: &mut Engine| {
        e.mem.set8(iv::OPEN, 0);
        e.mem.set8(iv::DRAWN, 0);
    };
    if e.mem.i16(mo::Y) > 30 {
        close(e);
        e.mem.set16(iv::HOVER, 0);
        e.mem.set8(iv::LAST_HOVER, 0);
        if e.mem.u8(iv::HELD) != 1 {
            let v = e.mem.u8(mo::SAVED_VERB);
            e.mouse_action(v);
        }
    } else {
        e.mem.set8(mo::HOVER, 0);
        e.mem.set16(iv::HOVER, 0);
        hover_slots(e);
        if e.mem.u8(iv::LAST_HOVER) != (e.mem.u16(iv::HOVER) & 0xff) {
            let h = e.mem.u16(iv::HOVER);
            e.mem.set8(iv::LAST_HOVER, h);
        } else if e.button(0) {
            let (hx, _) = e.hotspot();
            let first = e.mem.u8(iv::FIRST);
            let hov = e.mem.u16(iv::HOVER);
            if (0..=13).contains(&hx) && e.mem.u8(iv::OPEN) != 0 {
                if first > 0 {
                    e.mem.set8(iv::FIRST, first - 1);
                }
                return;
            }
            if (307..=320).contains(&hx)
                && e.mem.u8(iv::OPEN) != 0
                && first + 8 < 30
                && inv_slot(e, first as usize + 8).0
            {
                e.mem.set8(iv::FIRST, first + 1);
                return;
            }
            if hov != 0 && e.mem.u8(mo::VERB) == 1 {
                close(e);
                let v = e.mem.u8(mo::SAVED_VERB);
                e.mouse_action(v);
                speak(e, hov + 999);
                e.mem.set8(iv::HELD, 0);
                e.mem.set16(iv::HOVER, 0);
                e.mem.set8(iv::LAST_HOVER, 0);
            }
            if hov != 0 && e.mem.u8(mo::VERB) == 3 {
                let obj = inv_slot(e, (hov - 1) as usize).1;
                if e.mem.u8(iv::HELD) != 0 {
                    let m = &mut e.mem;
                    m.set8(iv::SECOND, hov - 1);
                    m.set8(iv::SECOND_OBJ, obj);
                    m.set8(iv::PENDING, 1);
                    m.set8(b::STATE, 2);
                    close(e);
                    e.mem.set16(iv::HOVER, 0);
                    e.mem.set8(iv::LAST_HOVER, 0);
                }
                if e.mem.u8(iv::HELD) != 1 {
                    let m = &mut e.mem;
                    m.set8(iv::HELD_SLOT, hov - 1);
                    m.set8(iv::HELD_OBJ, obj);
                    m.set8(iv::HELD, 1);
                    m.set8(b::STATE, 2);
                }
            }
        }
    }
    build_label(e);
    if e.button(1) && e.mem.u8(0xfd03) == 1 {
        let v = if e.mem.u8(mo::VERB) == 1 { 3 } else { 1 };
        e.mouse_action(v);
    }
}
