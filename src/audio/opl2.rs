//! Compact Yamaha YM3812 (OPL2) emulator.
//!
//! Structure follows the real chip (and Nuked-OPL3's organisation): 18
//! operators in 9 two-operator channels, 10-bit phase per cycle, operator
//! outputs in 13-bit units used directly as phase modulation, feedback,
//! KSL/TL/AM/VIB, four waveforms and the rhythm section (bass drum, snare,
//! tom, cymbal, hi-hat) with its noise generator.  Envelopes are modelled in
//! floating point (attenuation in dB) with the chip's rate tables.

pub const OPL_RATE: f64 = 49716.0;

const MULT: [f64; 16] = [0.5, 1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 10., 12., 12., 15., 15.];
const KSL_ROM: [i32; 16] = [0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64];
const KSL_SHIFT: [i32; 4] = [8, 1, 2, 0];
const CH_SLOTS: [[usize; 2]; 9] = [[0, 3], [1, 4], [2, 5], [6, 9], [7, 10], [8, 11], [12, 15], [13, 16], [14, 17]];
const SLOT_REG: [usize; 18] = [0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21];

fn reg_slot(lo: usize) -> Option<usize> {
    SLOT_REG.iter().position(|&r| r == lo)
}

fn channel_of_slot(s: usize) -> usize {
    CH_SLOTS.iter().position(|c| c[0] == s || c[1] == s).unwrap_or(0)
}

fn decay_db_per_sample(rate: i32) -> f64 {
    if rate < 4 {
        return 0.0;
    }
    let rate = rate.min(60);
    let t = 39.28 / (2f64.powi((rate >> 2) - 1) * (1.0 + (rate & 3) as f64 / 4.0));
    96.0 / (t * OPL_RATE)
}

fn attack_coef(rate: i32) -> f64 {
    if rate < 4 {
        return 0.0;
    }
    if rate >= 60 {
        return 1.0;
    }
    let t = 2.826 / (2f64.powi((rate >> 2) - 1) * (1.0 + (rate & 3) as f64 / 4.0));
    1.0 - (-6.0 / (t * OPL_RATE)).exp()
}

#[derive(Clone, Copy, PartialEq)]
enum Eg {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone)]
struct Slot {
    am: u8,
    vib: u8,
    eg_type: u8,
    ksr: u8,
    mult: f64,
    ksl: usize,
    tl: u8,
    ar: i32,
    dr: i32,
    sl: u8,
    rr: i32,
    wf: u8,
    phase: f64,
    env: f64,
    state: Eg,
    key: u8,
    out: f64,
    prev_out: f64,
    attack_k: f64,
    decay_k: f64,
    release_k: f64,
    ksl_att: f64,
}

impl Default for Slot {
    fn default() -> Self {
        Slot {
            am: 0,
            vib: 0,
            eg_type: 0,
            ksr: 0,
            mult: 1.0,
            ksl: 0,
            tl: 0,
            ar: 0,
            dr: 0,
            sl: 0,
            rr: 0,
            wf: 0,
            phase: 0.0,
            env: 96.0,
            state: Eg::Off,
            key: 0,
            out: 0.0,
            prev_out: 0.0,
            attack_k: 0.0,
            decay_k: 0.0,
            release_k: 0.0,
            ksl_att: 0.0,
        }
    }
}

#[derive(Clone, Default)]
struct Channel {
    fnum: i32,
    block: i32,
    key: u8,
    fb: i32,
    cnt: u8,
}

pub struct Opl2 {
    slots: Vec<Slot>,
    channels: Vec<Channel>,
    pub regs: [u8; 256],
    wse: bool,
    rhythm: u8,
    noise: u32,
    lfo_am: f64,
    lfo_vib: f64,
    sample_count: u64,
    sine: Vec<f64>,
}

impl Default for Opl2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Opl2 {
    pub fn new() -> Self {
        let sine = (0..1024).map(|i| ((i as f64 + 0.5) * std::f64::consts::PI / 512.0).sin()).collect();
        Opl2 {
            slots: vec![Slot::default(); 18],
            channels: vec![Channel::default(); 9],
            regs: [0; 256],
            wse: false,
            rhythm: 0,
            noise: 1,
            lfo_am: 0.0,
            lfo_vib: 0.0,
            sample_count: 0,
            sine,
        }
    }

    pub fn reset(&mut self) {
        for r in 0..256 {
            self.write(r, 0);
        }
    }

    fn wave(&self, kind: u8, phase: i32) -> f64 {
        let p = (phase & 1023) as usize;
        match kind {
            0 => self.sine[p],
            1 => {
                if p < 512 {
                    self.sine[p]
                } else {
                    0.0
                }
            }
            2 => self.sine[p].abs(),
            _ => {
                if p & 256 != 0 {
                    0.0
                } else {
                    self.sine[p].abs()
                }
            }
        }
    }

    pub fn write(&mut self, reg: usize, val: u8) {
        let reg = reg & 0xff;
        self.regs[reg] = val;
        let (hi, lo) = (reg & 0xe0, reg & 0x1f);
        if reg == 0x01 {
            self.wse = val & 0x20 != 0;
            return;
        }
        if reg == 0xbd {
            self.write_rhythm(val);
            return;
        }
        if (0x20..=0x80).contains(&hi) || hi == 0xe0 {
            let Some(s) = reg_slot(lo) else { return };
            let sl = &mut self.slots[s];
            match hi {
                0x20 => {
                    sl.am = val >> 7;
                    sl.vib = (val >> 6) & 1;
                    sl.eg_type = (val >> 5) & 1;
                    sl.ksr = (val >> 4) & 1;
                    sl.mult = MULT[(val & 15) as usize];
                }
                0x40 => {
                    sl.ksl = (val >> 6) as usize;
                    sl.tl = val & 63;
                }
                0x60 => {
                    sl.ar = (val >> 4) as i32;
                    sl.dr = (val & 15) as i32;
                }
                0x80 => {
                    sl.sl = val >> 4;
                    sl.rr = (val & 15) as i32;
                }
                0xe0 => sl.wf = val & 3,
                _ => {}
            }
            self.update_slot_rates(s);
            return;
        }
        if (0xa0..=0xa8).contains(&reg) {
            let n = reg - 0xa0;
            let c = &mut self.channels[n];
            c.fnum = (c.fnum & 0x300) | val as i32;
            self.update_channel(n);
        } else if (0xb0..=0xb8).contains(&reg) {
            let n = reg - 0xb0;
            let c = &mut self.channels[n];
            c.fnum = (c.fnum & 0xff) | (((val & 3) as i32) << 8);
            c.block = ((val >> 2) & 7) as i32;
            let key = (val >> 5) & 1;
            if key != c.key {
                c.key = key;
                for s in CH_SLOTS[n] {
                    self.key_slot(s, key != 0, 1);
                }
            }
            self.update_channel(n);
        } else if (0xc0..=0xc8).contains(&reg) {
            let c = &mut self.channels[reg - 0xc0];
            c.fb = ((val >> 1) & 7) as i32;
            c.cnt = val & 1;
        }
    }

    fn write_rhythm(&mut self, val: u8) {
        let old = self.rhythm;
        self.rhythm = val;
        let on = val & 0x20 != 0;
        // bit 4 BD (slots 12,15), 3 SD (16), 2 TOM (14), 1 CYM (17), 0 HH (13)
        let map: [&[usize]; 5] = [&[13], &[17], &[14], &[16], &[12, 15]];
        for (b, slots) in map.iter().enumerate() {
            let k = on && (val >> b) & 1 != 0;
            let ko = old & 0x20 != 0 && (old >> b) & 1 != 0;
            if k != ko {
                for &s in slots.iter() {
                    self.key_slot(s, k, 2);
                }
            }
        }
    }

    fn key_slot(&mut self, s: usize, on: bool, src: u8) {
        let sl = &mut self.slots[s];
        let before = sl.key;
        if on {
            sl.key |= src
        } else {
            sl.key &= !src
        }
        if before == 0 && sl.key != 0 {
            sl.state = Eg::Attack;
            sl.phase = 0.0;
        } else if before != 0 && sl.key == 0 {
            sl.state = Eg::Release;
        }
    }

    fn update_channel(&mut self, n: usize) {
        for s in CH_SLOTS[n] {
            self.update_slot_rates(s);
        }
    }

    fn update_slot_rates(&mut self, s: usize) {
        let c = self.channels[channel_of_slot(s)].clone();
        let sl = &mut self.slots[s];
        let keycode = (c.block << 1) | ((c.fnum >> 9) & 1);
        let rof = if sl.ksr != 0 { keycode } else { keycode >> 2 };
        let eff = |r: i32| if r != 0 { (r * 4 + rof).min(63) } else { 0 };
        sl.attack_k = attack_coef(eff(sl.ar));
        sl.decay_k = decay_db_per_sample(eff(sl.dr));
        sl.release_k = decay_db_per_sample(eff(sl.rr));
        let ksl = ((KSL_ROM[(c.fnum >> 6) as usize & 15] << 2) - ((8 - c.block) << 5)).max(0);
        sl.ksl_att = if sl.ksl != 0 { (ksl >> KSL_SHIFT[sl.ksl]) as f64 * 0.1875 } else { 0.0 };
    }

    fn envelope(&mut self, s: usize) -> f64 {
        let (lfo_am, deep_am) = (self.lfo_am, self.regs[0xbd] & 0x80 != 0);
        let sl = &mut self.slots[s];
        match sl.state {
            Eg::Attack => {
                if sl.attack_k >= 1.0 {
                    sl.env = 0.0;
                } else {
                    sl.env -= sl.env * sl.attack_k + if sl.attack_k != 0.0 { 0.0005 } else { 0.0 };
                }
                if sl.env <= 0.0 {
                    sl.env = 0.0;
                    sl.state = Eg::Decay;
                }
            }
            Eg::Decay => {
                let sus = if sl.sl == 15 { 93.0 } else { sl.sl as f64 * 3.0 };
                sl.env += sl.decay_k;
                if sl.env >= sus {
                    sl.env = sus;
                    sl.state = Eg::Sustain;
                }
            }
            Eg::Sustain => {
                if sl.eg_type == 0 {
                    sl.env += sl.release_k;
                }
            }
            Eg::Release => sl.env += sl.release_k,
            Eg::Off => {}
        }
        if sl.env >= 96.0 {
            sl.env = 96.0;
            if sl.state == Eg::Release || sl.state == Eg::Sustain {
                sl.state = Eg::Off;
            }
        }
        let mut att = sl.env + sl.tl as f64 * 0.75 + sl.ksl_att;
        if sl.am != 0 {
            att += lfo_am * if deep_am { 4.8 } else { 1.0 };
        }
        if att >= 96.0 { 0.0 } else { 10f64.powf(-att / 20.0) }
    }

    fn advance_phase(&mut self, s: usize, ch: usize) {
        let (fnum, block) = (self.channels[ch].fnum, self.channels[ch].block);
        let deep = self.regs[0xbd] & 0x40 != 0;
        let vib = self.lfo_vib;
        let sl = &mut self.slots[s];
        let mut f = fnum as f64;
        if sl.vib != 0 {
            let depth = if deep { 1.0 } else { 2.0 };
            f += vib * (fnum >> 7) as f64 / depth;
        }
        sl.phase = (sl.phase + f * (1u32 << block) as f64 * sl.mult) % (1u32 << 20) as f64;
    }

    fn ph(&self, s: usize) -> i32 {
        ((self.slots[s].phase as i64 >> 10) & 1023) as i32
    }

    fn op(&mut self, s: usize, phase: i32) -> f64 {
        let amp = self.envelope(s);
        let wf = if self.wse { self.slots[s].wf } else { 0 };
        if amp != 0.0 { self.wave(wf, phase) * amp } else { 0.0 }
    }

    /// Generate samples (mono, roughly -1..1) at [`OPL_RATE`].
    pub fn generate(&mut self, out: &mut [f32]) {
        let rhythm_on = self.rhythm & 0x20 != 0;
        for o in out.iter_mut() {
            self.sample_count += 1;
            let t = self.sample_count as f64 / OPL_RATE;
            self.lfo_am = (1.0 - (2.0 * std::f64::consts::PI * 3.7 * t).cos()) / 2.0;
            self.lfo_vib = (2.0 * std::f64::consts::PI * 6.07 * t).sin();
            let bit = ((self.noise >> 22) ^ (self.noise >> 8)) & 1;
            self.noise = ((self.noise << 1) | bit) & 0x7fffff;
            let mut mix = 0.0;
            let last = if rhythm_on { 6 } else { 9 };
            for ch in 0..last {
                mix += self.channel_out(ch);
            }
            if rhythm_on {
                mix += self.rhythm_out();
            }
            *o = (mix * 0.14).tanh() as f32;
        }
    }

    fn fb_phase(&self, s: usize, fb: i32) -> i32 {
        if fb == 0 {
            return 0;
        }
        let sl = &self.slots[s];
        ((sl.out + sl.prev_out) * 2f64.powi(fb - 7) * 1024.0).round() as i32
    }

    fn channel_out(&mut self, ch: usize) -> f64 {
        let [ms, cs] = CH_SLOTS[ch];
        self.advance_phase(ms, ch);
        self.advance_phase(cs, ch);
        if self.slots[ms].state == Eg::Off && self.slots[cs].state == Eg::Off {
            self.slots[ms].out = 0.0;
            self.slots[ms].prev_out = 0.0;
            return 0.0;
        }
        let fb = self.fb_phase(ms, self.channels[ch].fb);
        let p = self.ph(ms) + fb;
        let mo = self.op(ms, p);
        let m = &mut self.slots[ms];
        m.prev_out = m.out;
        m.out = mo;
        let pc = self.ph(cs);
        if self.channels[ch].cnt != 0 { mo + self.op(cs, pc) } else { self.op(cs, pc + (mo * 4096.0).round() as i32) }
    }

    fn rhythm_out(&mut self) -> f64 {
        for s in [12, 15] {
            self.advance_phase(s, 6);
        }
        self.advance_phase(13, 7);
        self.advance_phase(16, 7);
        self.advance_phase(14, 8);
        self.advance_phase(17, 8);
        let mut out = 0.0;
        let fb = self.fb_phase(12, self.channels[6].fb);
        let p = self.ph(12) + fb;
        let mo = self.op(12, p);
        self.slots[12].prev_out = self.slots[12].out;
        self.slots[12].out = mo;
        let p15 = self.ph(15);
        out += 2.0
            * if self.channels[6].cnt != 0 {
                self.op(15, p15)
            } else {
                self.op(15, p15 + (mo * 4096.0).round() as i32)
            };
        let (p13, p17) = (self.ph(13), self.ph(17));
        let b = |p: i32, n: i32| (p >> n) & 1;
        let rm_xor = (b(p13, 2) ^ b(p13, 7)) | (b(p13, 3) ^ b(p17, 5)) | (b(p17, 3) ^ b(p17, 5));
        let noise = (self.noise & 1) as i32;
        let ph_hh = (rm_xor << 9) | if rm_xor ^ noise != 0 { 0xd0 } else { 0x34 };
        out += 2.0 * self.op(13, ph_hh);
        let ph_sd = (b(p13, 8) << 9) | ((b(p13, 8) ^ noise) << 8);
        out += 2.0 * self.op(16, ph_sd);
        let pt = self.ph(14);
        out += 2.0 * self.op(14, pt);
        out += 2.0 * self.op(17, (rm_xor << 9) | 0x80);
        out
    }
}
