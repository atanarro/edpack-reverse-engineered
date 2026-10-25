//! Emulation of the original interpreter's data segment (DGROUP).
//!
//! RUNDAT.EXE keeps nearly all game state in global variables, and the script
//! language exposes many of them as "variables" through a table of far
//! pointers (var n -> 16-bit word at some address).  Several of those pointers
//! overlap byte-sized fields, so writing a variable can clobber a neighbouring
//! field.  To reproduce that exactly, state lives in a 64 KiB byte array laid
//! out like the original DS segment and is accessed by the original offsets.

/// Addresses of the level data blocks and core globals inside DS.
pub mod ds {
    pub const GFX_NAMES: usize = 0x43fa; // 4 x char[13]: scenario, drawings, movements, inventory PCX
    pub const FILE_NAMES: usize = 0x442e; // 20 x char[13]
    pub const LIMITMOV: usize = 0x6c72; // 320 x 22 bytes
    pub const LIMIT: usize = 0x87f2; // 40 x 14 bytes
    pub const THINGS: usize = 0x8b67; // 30 x 92 bytes
    pub const THING_ORDER: usize = 0x8b49; // 30 bytes
    pub const DOORS: usize = 0x685e; // 20 x 10 bytes
    pub const TEXT_POOL: usize = 0x9823; // 25000 bytes
    pub const TEXT_OFFS: usize = 0x962f; // 250 x i16
    pub const ACTION_OFFS: usize = 0x6926; // 50 x i16
    pub const ANIMS: usize = 0x1dd0; // 50 x 10 bytes
    pub const ACINVTH: usize = 0x6b82; // 30 x 8 bytes
    pub const COORANIM: usize = 0x4532; // 100 x 10 bytes
    pub const SELECTEXT: usize = 0x40dc; // 50 x 12 bytes
    pub const MESSAGES: usize = 0x3bfa; // 5 x 250 bytes
    pub const SCALEMOV: usize = 0x389a; // 20 x 18 bytes
    pub const SHADOWMOV: usize = 0x37aa; // 20 x 12 bytes
    pub const DEFFRAMES: usize = 0x1194; // 50 x 52 bytes
    pub const FILE_TAB: usize = 0x435a; // 20 x {u32 off, u32 size}
    pub const GFX_TAB: usize = 0x433a; // 4 x {u32 off, u32 size}
    pub const DRAWANIM_TAB: usize = 0x3a02; // 25 x 20 bytes

    pub const LEVEL: usize = 0xfc9c; // u8 current level
    pub const NEXT_DOOR: usize = 0xfcd1;
    pub const NEXT_MODE: usize = 0xfcd2;
    pub const CUR_ACTION: usize = 0xfcc9; // u8
    pub const CUR_ACTION_PC: usize = 0xfcca; // u8
}

pub const THING_SIZE: usize = 92;

/// Variable n -> DS address of its 16-bit word (variables 0..46).
pub const GLOBAL_VAR_ADDR: [usize; 47] = [
    0xfc19, 0xfbf5, 0xfc01, 0xfc03, 0xfc0d, 0xfc0f, 0xfc1b, 0xfca9, 0xfcd4, 0xfcd6, 0xfcda, 0x8a25, 0x8a27, 0xfce0,
    0xfce2, 0xfce4, 0xfce6, 0xfce8, 0xfcea, 0xfcec, 0xfcee, 0xfd05, 0xfd20, 0xfd22, 0x8a2d, 0x00dc, 0xfcf2, 0xfcf4,
    0xfd04, 0xfd01, 0xfcff, 0xfcfe, 0xfd03, 0xfbdf, 0xfbeb, 0xfbed, 0xfbe1, 0xfbe3, 0xfbe5, 0xfbe7, 0xfbe9, 0xfd3c,
    0xfd3e, 0xfbdd, 0xfd30, 0xfd32, 0xfd34,
];

/// THING variable v (0..29) -> byte offset inside the THING record.
const THING_VAR_OFFSET: [(usize, usize); 21] = [
    (0, 0x2b),
    (1, 0x15),
    (2, 0x21),
    (3, 0x23),
    (5, 0x19),
    (6, 0x1b),
    (7, 0x2d),
    (8, 0x40),
    (9, 0x42),
    (10, 0x44),
    (11, 0x09),
    (12, 0x0b),
    (15, 0x48),
    (16, 0x4a),
    (17, 0x4c),
    (18, 0x4e),
    (19, 0x50),
    (20, 0x00),
    (21, 0x02),
    (22, 0x04),
    (23, 0x06),
];

/// Variables 1000..1999 live in a separate buffer (they persist between levels).
pub const USER_VARS_BASE: usize = 0x10000;
/// Variables without a pointer in the original table point at the same
/// uninitialised far pointer; they share one scratch word here.
pub const SCRATCH_VAR_ADDR: usize = USER_VARS_BASE + 2000 * 2;
/// State that later RUNDAT builds keep and the demo's data segment has no
/// room for (see [`v102`]).
pub const EXT_BASE: usize = SCRATCH_VAR_ADDR + 16;
pub const MEMORY_SIZE: usize = EXT_BASE + 0x100;

/// RUNDAT v1.02 additions, in the extension area
/// (docs/reverse-engineering/07-rundat-versions.md §4).
pub mod v102 {
    use super::EXT_BASE;
    /// 50 x i16: the animations' skip targets (record word +0x0a), -1 = none.
    pub const ANIM_SKIP: usize = EXT_BASE;
    /// i16: skip target of the running action (SKIPTO outside animations).
    pub const SKIP_TARGET: usize = EXT_BASE + 100;
    /// u8: cycles 0..100 at 50 Hz, read by RAND.
    pub const RAND_COUNTER: usize = EXT_BASE + 102;
    /// u8: the verb last used inside the inventory panel.
    pub const INV_VERB: usize = EXT_BASE + 103;
    pub const VAR47_SKIP_KEY: usize = EXT_BASE + 104;
    pub const VAR48_ISANIM: usize = EXT_BASE + 106;
    pub const VAR50_VERBS: usize = EXT_BASE + 108;
    pub const VAR51_LOOK: usize = EXT_BASE + 110;
    pub const VAR52_USE: usize = EXT_BASE + 112;
    pub const VAR53_MENU_CURSOR: usize = EXT_BASE + 114;
    pub const VAR54_ARROW_CURSOR: usize = EXT_BASE + 116;
    /// vars 28..32 as independent words (the demo's byte flags fd04, fd01, fcff, fcfe, fd03).
    pub const VAR28_MENU: usize = EXT_BASE + 118;
    pub const VAR29_BAR: usize = EXT_BASE + 120;
    pub const VAR30_HOVER: usize = EXT_BASE + 122;
    pub const VAR31_INV: usize = EXT_BASE + 124;
    pub const VAR32_VERBS: usize = EXT_BASE + 126;
    /// cursor table entries 6..9 (12 bytes each; 0..5 stay at the demo's table).
    pub const CURSORS_6_9: usize = EXT_BASE + 128;
}

#[derive(Clone)]
pub struct Memory {
    pub bytes: Vec<u8>,
    var_addr: Vec<usize>,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Memory::with_vars(Self::v2_vars())
    }

    /// The variable table of RUNDAT v1.0 (shareware and registered).
    pub fn v2_vars() -> Vec<usize> {
        let mut var_addr = vec![SCRATCH_VAR_ADDR; 2000];
        for (n, &a) in GLOBAL_VAR_ADDR.iter().enumerate() {
            var_addr[n] = a;
        }
        for t in 0..30 {
            for &(v, off) in &THING_VAR_OFFSET {
                var_addr[100 + t * 30 + v] = ds::THINGS + t * THING_SIZE + off;
            }
        }
        var_addr
    }

    /// RUNDAT v1.02: vars 28..32 become separate words, 47..54 are new
    /// (49 is the walking flag at fcce).
    pub fn v102_vars() -> Vec<usize> {
        let mut v = Self::v2_vars();
        v[28] = v102::VAR28_MENU;
        v[29] = v102::VAR29_BAR;
        v[30] = v102::VAR30_HOVER;
        v[31] = v102::VAR31_INV;
        v[32] = v102::VAR32_VERBS;
        v[47] = v102::VAR47_SKIP_KEY;
        v[48] = v102::VAR48_ISANIM;
        v[49] = 0xfcce;
        v[50] = v102::VAR50_VERBS;
        v[51] = v102::VAR51_LOOK;
        v[52] = v102::VAR52_USE;
        v[53] = v102::VAR53_MENU_CURSOR;
        v[54] = v102::VAR54_ARROW_CURSOR;
        v
    }

    /// Memory with a custom variable table (variable n -> address of its
    /// word); variables 1000..1999 always map to the user-variable buffer.
    pub fn with_vars(mut var_addr: Vec<usize>) -> Self {
        var_addr.resize(2000, SCRATCH_VAR_ADDR);
        for (n, slot) in var_addr.iter_mut().enumerate().skip(1000) {
            *slot = USER_VARS_BASE + (n - 1000) * 2;
        }
        Memory { bytes: vec![0; MEMORY_SIZE], var_addr }
    }

    /// Address of variable n's word.
    pub fn var_addr(&self, n: i32) -> Option<usize> {
        self.var_addr.get(usize::try_from(n).ok()?).copied()
    }

    #[inline]
    pub fn u8(&self, a: usize) -> i32 {
        self.bytes.get(a).copied().unwrap_or(0) as i32
    }
    #[inline]
    pub fn i8(&self, a: usize) -> i32 {
        self.bytes.get(a).copied().unwrap_or(0) as i8 as i32
    }
    #[inline]
    pub fn set8(&mut self, a: usize, v: i32) {
        if let Some(b) = self.bytes.get_mut(a) {
            *b = v as u8;
        }
    }
    #[inline]
    pub fn u16(&self, a: usize) -> i32 {
        self.u8(a) | (self.u8(a + 1) << 8)
    }
    #[inline]
    pub fn i16(&self, a: usize) -> i32 {
        self.u16(a) as u16 as i16 as i32
    }
    #[inline]
    pub fn set16(&mut self, a: usize, v: i32) {
        self.set8(a, v & 0xff);
        self.set8(a + 1, (v >> 8) & 0xff);
    }
    pub fn u32(&self, a: usize) -> u32 {
        self.u16(a) as u32 | ((self.u16(a + 2) as u32) << 16)
    }

    pub fn fill(&mut self, a: usize, n: usize, v: u8) {
        self.bytes[a..a + n].fill(v);
    }
    pub fn write(&mut self, a: usize, src: &[u8]) {
        self.bytes[a..a + src.len()].copy_from_slice(src);
    }
    pub fn slice(&self, a: usize, n: usize) -> Vec<u8> {
        self.bytes[a..a + n].to_vec()
    }

    /// NUL-terminated byte string at `a` (at most `max` bytes, code page 850).
    pub fn cstr(&self, a: usize, max: usize) -> Vec<u8> {
        let end = (a + max).min(self.bytes.len());
        let s = &self.bytes[a.min(end)..end];
        let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        s[..n].to_vec()
    }

    pub fn get_var(&self, n: i32) -> i32 {
        if !(0..2000).contains(&n) {
            return 0;
        }
        self.i16(self.var_addr[n as usize])
    }
    pub fn set_var(&mut self, n: i32, v: i32) {
        if !(0..2000).contains(&n) {
            return;
        }
        let a = self.var_addr[n as usize];
        self.set16(a, v);
    }
}
