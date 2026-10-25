//! The bytecode: opcodes, argument counts (from ED.EXE's command table
//! ED.DAT) and a disassembler.  The interpreter itself is
//! [`crate::engine::Engine::run_script`], a port of RUNDAT.EXE 0x2ca2.
//!
//! A script is a stream of 16-bit words; the program counter is an 8-bit word
//! offset relative to the script start.  Arguments >= 25000 are variable
//! references (dereferenced for the first four arguments).

pub const VAR_REF: i32 = 25000;
/// More consecutive ops without yielding is a fatal error in the original.
pub const MAX_OPS: u32 = 51;

/// (opcode, name, argument count)
pub const OPS: &[(i32, &str, usize)] = &[
    (0, "END", 0),
    (1, "GOLEVEL", 3),
    (2, "GOXY", 2),
    (3, "SPEAK", 1),
    (4, "PTEXT", 4),
    (5, "ACTION", 1),
    (7, "VAR", 2),
    (8, "VARVAR", 2),
    (9, "ADD", 2),
    (10, "ADDVAR", 2),
    (11, "SUB", 2),
    (12, "SUBVAR", 2),
    (13, "ANIM_ON", 1),
    (14, "ANIM_OFF", 1),
    (15, "RET", 0),
    (16, "JMP", 1),
    (17, "CMP", 2),
    (19, "JE", 1),
    (20, "JNE", 1),
    (21, "JL", 1),
    (22, "JA", 1),
    (23, "JLE", 1),
    (24, "JAE", 1),
    (25, "ADDINV", 1),
    (26, "SUBINV", 1),
    (27, "SHADOW", 2),
    (28, "MODPAL", 3),
    (29, "RPAL", 2),
    (30, "LPAL", 2),
    (31, "NEWTHING", 2),
    (32, "SHOWPCX", 3),
    (33, "LOADPCX", 2),
    (34, "DRAW", 0),
    (35, "SPEEDXY", 2),
    (36, "PLAYCMF", 1),
    (37, "STOPCMF", 0),
    (38, "PLAYVOC", 0),
    (39, "PPOS", 1),
    (40, "GETVOC", 1),
    (41, "EXIT", 0),
    (42, "STEXT", 3),
    (43, "SCALEXY", 2),
    (44, "MOUSEACTION", 1),
    (45, "PMOV", 1),
    (46, "MMOUSE", 1),
    (52, "MENU", 0),
    (53, "DEFSCR", 2),
    (54, "RAND", 2),
    (55, "SEEINV", 1),
    (56, "NEWMOVS", 1),
    (57, "DRAWANIM", 3),
    (58, "LOADRGB", 2),
    (60, "PUTPAL", 0),
    (61, "NEWMOUSE", 1),
    (62, "PLAYFLI", 3),
    (63, "FIRSTFRAME", 2),
    (64, "NEXTFRAME", 2),
    (65, "DELAY", 2),
    (66, "LOADDATA", 2),
    // source-level aliases accepted by the compiler
    (0, "FINISH", 0),
    (44, "MOUSEACT", 1),
];

pub fn op_info(op: i32) -> Option<(&'static str, usize)> {
    OPS.iter().find(|o| o.0 == op).map(|o| (o.1, o.2))
}

pub fn opcode_of(name: &str) -> Option<i32> {
    OPS.iter().find(|o| o.1 == name).map(|o| o.0)
}

pub mod op {
    pub const END: i32 = 0;
    pub const GOLEVEL: i32 = 1;
    pub const GOXY: i32 = 2;
    pub const SPEAK: i32 = 3;
    pub const PTEXT: i32 = 4;
    pub const ACTION: i32 = 5;
    pub const VAR: i32 = 7;
    pub const VARVAR: i32 = 8;
    pub const ADD: i32 = 9;
    pub const ADDVAR: i32 = 10;
    pub const SUB: i32 = 11;
    pub const SUBVAR: i32 = 12;
    pub const ANIM_ON: i32 = 13;
    pub const ANIM_OFF: i32 = 14;
    pub const RET: i32 = 15;
    pub const JMP: i32 = 16;
    pub const CMP: i32 = 17;
    pub const JE: i32 = 19;
    pub const JNE: i32 = 20;
    pub const JL: i32 = 21;
    pub const JA: i32 = 22;
    pub const JLE: i32 = 23;
    pub const JAE: i32 = 24;
    pub const ADDINV: i32 = 25;
    pub const SUBINV: i32 = 26;
    pub const SHADOW: i32 = 27;
    pub const MODPAL: i32 = 28;
    pub const RPAL: i32 = 29;
    pub const LPAL: i32 = 30;
    pub const NEWTHING: i32 = 31;
    pub const SHOWPCX: i32 = 32;
    pub const LOADPCX: i32 = 33;
    pub const DRAW: i32 = 34;
    pub const SPEEDXY: i32 = 35;
    pub const PLAYCMF: i32 = 36;
    pub const STOPCMF: i32 = 37;
    pub const PLAYVOC: i32 = 38;
    pub const PPOS: i32 = 39;
    pub const GETVOC: i32 = 40;
    pub const EXIT: i32 = 41;
    pub const STEXT: i32 = 42;
    pub const SCALEXY: i32 = 43;
    pub const MOUSEACTION: i32 = 44;
    pub const PMOV: i32 = 45;
    pub const MMOUSE: i32 = 46;
    pub const MENU: i32 = 52;
    pub const DEFSCR: i32 = 53;
    pub const RAND: i32 = 54;
    pub const SEEINV: i32 = 55;
    pub const NEWMOVS: i32 = 56;
    pub const DRAWANIM: i32 = 57;
    pub const LOADRGB: i32 = 58;
    pub const PUTPAL: i32 = 60;
    pub const NEWMOUSE: i32 = 61;
    pub const PLAYFLI: i32 = 62;
    pub const FIRSTFRAME: i32 = 63;
    pub const NEXTFRAME: i32 = 64;
    pub const DELAY: i32 = 65;
    pub const LOADDATA: i32 = 66;
}

/// Script results.  DELAY returns its counter's low byte (any 0..255
/// value; 0 makes the caller think the script ended, an original quirk).
pub const FINISHED: i32 = 0;
pub const YIELD: i32 = 1;

/// Disassemble a script (debugging tools).
pub fn disassemble(code: &[i16], base: usize, max_words: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut pc = 0;
    let w = |i: usize| code.get(i).copied().unwrap_or(0) as i32;
    while pc < max_words {
        let op = w(base + pc);
        let Some((name, argc)) = op_info(op) else {
            out.push(format!("{pc}: ?{op}"));
            break;
        };
        let args: Vec<String> = (1..=argc)
            .map(|i| {
                let a = w(base + pc + i);
                if a >= VAR_REF { format!("[{}]", a - VAR_REF) } else { a.to_string() }
            })
            .collect();
        out.push(format!("{pc}: {name}({})", args.join(",")));
        if op == 0 {
            break;
        }
        pc += 1 + argc;
    }
    out
}
