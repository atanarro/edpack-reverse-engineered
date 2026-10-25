//! Creative Music File (CMF) player driving the OPL2 emulator, modelled on
//! the behaviour of Creative's SBFMDRV.COM driver used by EDPACK.

use super::opl2::{OPL_RATE, Opl2};
use crate::Error;

const MOD_SLOT_REG: [usize; 9] = [0, 1, 2, 8, 9, 10, 16, 17, 18];

#[derive(Clone, Copy, PartialEq)]
enum Ops {
    Both,
    Car,
    Mod,
}

/// Rhythm MIDI channels 11..15 -> (OPL channel, operators, rhythm bit).
fn drum(ch: usize) -> (usize, Ops, u8) {
    match ch {
        11 => (6, Ops::Both, 0x10), // bass drum
        12 => (7, Ops::Car, 0x08),  // snare
        13 => (8, Ops::Mod, 0x04),  // tom-tom
        14 => (8, Ops::Car, 0x02),  // cymbal
        _ => (7, Ops::Mod, 0x01),   // hi-hat
    }
}

pub struct Song {
    instruments: Vec<[u8; 16]>,
    music: Vec<u8>,
    ticks_per_second: f64,
}

pub fn parse_cmf(bytes: &[u8]) -> Result<Song, Error> {
    if bytes.len() < 40 || &bytes[..4] != b"CTMF" {
        return Err(Error::new("No es un archivo CMF"));
    }
    let w = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
    let (ins_off, mus_off) = (w(6), w(8));
    let tps = match w(12) {
        0 => 96,
        t => t,
    };
    let instruments = (0..w(36))
        .map(|i| {
            let mut ins = [0u8; 16];
            for (k, b) in ins.iter_mut().enumerate() {
                *b = bytes.get(ins_off + i * 16 + k).copied().unwrap_or(0);
            }
            ins
        })
        .collect();
    Ok(Song { instruments, music: bytes.get(mus_off..).unwrap_or(&[]).to_vec(), ticks_per_second: tps as f64 })
}

fn note_to_freq(note: i32) -> (u8, i32) {
    let f = 440.0 * 2f64.powf((note - 69) as f64 / 12.0);
    let mut block = 0;
    let mut fnum = (f * (1 << 20) as f64 / OPL_RATE).round() as i32;
    while fnum >= 1024 && block < 7 {
        block += 1;
        fnum = (f * (1 << (20 - block)) as f64 / OPL_RATE).round() as i32;
    }
    (block as u8, fnum.min(1023))
}

pub struct CmfPlayer {
    pub opl: Opl2,
    song: Option<Song>,
    pub playing: bool,
    pub looping: bool,
    pos: usize,
    wait: i64,
    status: u8,
    rhythm_mode: bool,
    transpose: i32,
    ch_note: [i32; 9],
    bd: u8,
    tick_acc: f64,
}

impl Default for CmfPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl CmfPlayer {
    pub fn new() -> Self {
        CmfPlayer {
            opl: Opl2::new(),
            song: None,
            playing: false,
            looping: false,
            pos: 0,
            wait: 0,
            status: 0,
            rhythm_mode: false,
            transpose: 0,
            ch_note: [-1; 9],
            bd: 0,
            tick_acc: 0.0,
        }
    }

    pub fn load(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.song = Some(parse_cmf(bytes)?);
        self.restart();
        Ok(())
    }

    pub fn has_song(&self) -> bool {
        self.song.is_some()
    }

    pub fn restart(&mut self) {
        self.opl.reset();
        self.opl.write(0x01, 0x20);
        self.pos = 0;
        self.status = 0;
        self.rhythm_mode = false;
        self.transpose = 0;
        self.ch_note = [-1; 9];
        self.bd = 0;
        self.playing = true;
        self.tick_acc = 0.0;
        self.wait = self.read_var() as i64;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        for c in 0..9 {
            let v = self.opl.regs[0xb0 + c] & !0x20;
            self.opl.write(0xb0 + c, v);
        }
        self.opl.write(0xbd, 0);
    }

    fn mb(&self, p: usize) -> u8 {
        self.song.as_ref().and_then(|s| s.music.get(p).copied()).unwrap_or(0)
    }
    fn music_len(&self) -> usize {
        self.song.as_ref().map(|s| s.music.len()).unwrap_or(0)
    }

    fn read_var(&mut self) -> u32 {
        let mut v = 0u32;
        loop {
            if self.pos >= self.music_len() {
                return 0;
            }
            let b = self.mb(self.pos);
            self.pos += 1;
            v = (v << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                return v;
            }
        }
    }

    fn set_instrument(&mut self, ch: usize, ins: [u8; 16], ops: Ops) {
        let r = MOD_SLOT_REG[ch];
        let regs = [0x20, 0x40, 0x60, 0x80, 0xe0];
        let idx = [0, 2, 4, 6, 8];
        for k in 0..5 {
            if ops == Ops::Both {
                self.opl.write(regs[k] + r, ins[idx[k]]);
                self.opl.write(regs[k] + r + 3, ins[idx[k] + 1]);
            } else {
                // single-operator drums take the modulator half of the instrument
                self.opl.write(regs[k] + r + if ops == Ops::Car { 3 } else { 0 }, ins[idx[k]]);
            }
        }
        if ops != Ops::Car {
            self.opl.write(0xc0 + ch, ins[10]);
        }
    }

    fn note_on(&mut self, ch: usize, note: i32) {
        let note = note + self.transpose;
        if self.rhythm_mode && ch >= 11 {
            let (c, _, bit) = drum(ch);
            let (block, fnum) = note_to_freq(note);
            self.opl.write(0xa0 + c, (fnum & 0xff) as u8);
            self.opl.write(0xb0 + c, (block << 2) | (fnum >> 8) as u8);
            self.bd &= !bit;
            self.opl.write(0xbd, 0x20 | self.bd);
            self.bd |= bit;
            self.opl.write(0xbd, 0x20 | self.bd);
            return;
        }
        if ch >= if self.rhythm_mode { 6 } else { 9 } {
            return;
        }
        let (block, fnum) = note_to_freq(note);
        let v = self.opl.regs[0xb0 + ch] & !0x20;
        self.opl.write(0xb0 + ch, v);
        self.opl.write(0xa0 + ch, (fnum & 0xff) as u8);
        self.opl.write(0xb0 + ch, 0x20 | (block << 2) | (fnum >> 8) as u8);
        self.ch_note[ch] = note;
    }

    fn note_off(&mut self, ch: usize, note: i32) {
        let note = note + self.transpose;
        if self.rhythm_mode && ch >= 11 {
            let (_, _, bit) = drum(ch);
            self.bd &= !bit;
            self.opl.write(0xbd, 0x20 | self.bd);
            return;
        }
        if ch >= 9 || self.ch_note[ch] != note {
            return;
        }
        let v = self.opl.regs[0xb0 + ch] & !0x20;
        self.opl.write(0xb0 + ch, v);
        self.ch_note[ch] = -1;
    }

    fn program_change(&mut self, ch: usize, p: usize) {
        let Some(song) = &self.song else { return };
        let Some(&ins) = song.instruments.get(p).or(song.instruments.first()) else { return };
        if self.rhythm_mode && ch >= 11 {
            let (c, ops, _) = drum(ch);
            self.set_instrument(c, ins, ops);
        } else if ch < 9 {
            self.set_instrument(ch, ins, Ops::Both);
        }
    }

    fn event(&mut self) -> bool {
        let mut st = self.mb(self.pos);
        if st & 0x80 != 0 {
            self.pos += 1;
            if st < 0xf0 {
                self.status = st;
            }
        } else {
            st = self.status;
        }
        let (kind, ch) = (st & 0xf0, (st & 15) as usize);
        let next = |s: &mut Self| {
            let b = s.mb(s.pos);
            s.pos += 1;
            b
        };
        match kind {
            0x80 => {
                let n = next(self) as i32;
                self.pos += 1;
                self.note_off(ch, n);
            }
            0x90 => {
                let n = next(self) as i32;
                let vel = next(self);
                if vel != 0 { self.note_on(ch, n) } else { self.note_off(ch, n) }
            }
            0xa0 | 0xe0 => self.pos += 2,
            0xb0 => {
                let c = next(self);
                let val = next(self);
                match c {
                    0x67 => {
                        self.rhythm_mode = val != 0;
                        self.bd = 0;
                        self.opl.write(0xbd, if self.rhythm_mode { 0x20 } else { 0 });
                    }
                    0x68 => self.transpose = val as i32,
                    0x69 => self.transpose = -(val as i32),
                    _ => {}
                }
            }
            0xc0 => {
                let p = next(self) as usize;
                self.program_change(ch, p);
            }
            0xd0 => self.pos += 1,
            0xf0 => {
                if st == 0xff {
                    let meta = next(self);
                    let len = self.read_var() as usize;
                    self.pos += len;
                    if meta == 0x2f {
                        return false;
                    }
                } else {
                    let len = self.read_var() as usize;
                    self.pos += len;
                }
            }
            _ => {}
        }
        self.pos < self.music_len()
    }

    /// Advance the sequencer by one MIDI tick.
    fn tick(&mut self) {
        if !self.playing {
            return;
        }
        while self.wait <= 0 {
            if !self.event() {
                if self.looping {
                    self.restart();
                } else {
                    self.stop();
                }
                return;
            }
            self.wait = self.read_var() as i64;
        }
        self.wait -= 1;
    }

    /// Render samples at OPL_RATE.
    pub fn render(&mut self, out: &mut [f32]) {
        let Some(tps) = self.song.as_ref().map(|s| s.ticks_per_second) else {
            out.fill(0.0);
            return;
        };
        let spt = OPL_RATE / tps;
        let mut i = 0;
        while i < out.len() {
            if self.tick_acc <= 0.0 {
                self.tick();
                self.tick_acc += spt;
            }
            let k = (out.len() - i).min(self.tick_acc.ceil() as usize).max(1);
            self.opl.generate(&mut out[i..i + k]);
            i += k;
            self.tick_acc -= k as f64;
        }
    }
}
