//! Player movement of RUNVGA v1: GOXY (0x7dab) and the walker (0x3e6a).
//! A literal port, bugs included (see docs/v1-runvga/04-player-render.md).

use crate::memory::Memory;

pub const PX: usize = 0x91dd;
pub const PY: usize = 0x91df;
pub const SRCX: usize = 0x91e5;
pub const SRCY: usize = 0x91e7;
pub const PW: usize = 0x91e9;
pub const PH: usize = 0x91eb;
pub const DIRX: usize = 0x91fd;
pub const DIRY: usize = 0x91fe;
pub const CUR: usize = 0x91ff;
pub const ANIM: usize = 0x91d9;
pub const FRAME: usize = 0x91da;
pub const LAST_FRAME: usize = 0x91db;
pub const SAVED: usize = 0x91dc;
pub const FLIP: usize = 0x91f7;
pub const VISIBLE: usize = 0x91f5;
pub const PATH: usize = 0x9204;
pub const WALKING: usize = 0x92a0;
pub const SPEAKING: usize = 0x9297;
pub const STEP_WAIT: usize = 0x92a1;
/// Ticks per walk step (SETUP.CFG speed).
pub const STEP_TICKS: usize = 0x3536;
pub const STEP_X: usize = 0x987;
pub const STEP_Y: usize = 0x986;
/// Frame size of the player sprite (from the level).
pub const FRAME_W: usize = 0x980;
pub const FRAME_H: usize = 0x982;
/// Frame source points in the gfx-2 sheet: 100 x {x, y}.
pub const FRAMES: usize = 0x96b0;

pub const S_BACK: usize = 0x9a;
pub const S_FRONT: usize = 0x9b;
pub const S_SIDE: usize = 0x9c;
pub const S_TSIDE: usize = 0x9d;
pub const S_TBACK: usize = 0x9e;
pub const S_TFRONT: usize = 0x9f;
pub const C_BACK: usize = 0xa1;
pub const C_FRONT: usize = 0xa2;
pub const C_SIDE: usize = 0xa3;
pub const C_TBACK: usize = 0xa4;
pub const C_TFRONT: usize = 0xa5;
pub const C_TSIDE: usize = 0xa6;

pub const LIMIT: usize = 0x3392;
pub const LIMITMOV: usize = 0x30fe;

/// C division by 2 (truncates toward zero).
fn half(v: i32) -> i32 {
    v / 2
}

fn feet_x(m: &Memory) -> i32 {
    m.i16(PX) + half(m.i16(PW))
}
fn feet_y(m: &Memory) -> i32 {
    m.i16(PY) + m.i16(PH)
}

/// GOXY(x, y): LIMIT redirection, LIMITMOV routing, start walking.
pub fn go_xy(m: &mut Memory, mut x: i32, mut y: i32) {
    for i in 0..30 {
        let l = LIMIT + 14 * i;
        if m.i16(l) == 0 {
            continue;
        }
        if m.i16(l + 2) <= x && m.i16(l + 4) <= y && m.i16(l + 6) >= x && m.i16(l + 8) >= y {
            let (tx, ty) = (m.i16(l + 10), m.i16(l + 12));
            if tx != -100 && tx != -101 {
                x = tx;
            }
            if tx == -101 {
                x = feet_x(m);
            }
            if ty != -100 && ty != -101 {
                y = ty;
            }
            if ty == -101 {
                y = feet_y(m);
            }
        }
    }
    let (mut cx, mut cy) = (feet_x(m), feet_y(m));
    // QUIRK: the original adds the sprite top twice, so this rarely fires.
    if cx == x && m.i16(PY) + m.i16(PY) + m.i16(PH) == y {
        return;
    }
    let (mut k, mut c) = (0, 0);
    while c < 30 {
        let r = LIMITMOV + 22 * c;
        let w = |o: usize| m.i16(r + o);
        if w(0) != 0
            && w(2) <= x
            && w(4) <= y
            && w(6) >= x
            && w(8) >= y
            && w(10) <= cx
            && w(14) >= cx
            && w(12) <= cy
            && w(16) >= cy
        {
            let (wx, wy) = (w(18), w(20));
            m.set16(PATH + 4 * k, wx);
            m.set16(PATH + 4 * k + 2, wy);
            (cx, cy) = (wx, wy);
            k += 1;
            c = 0;
            if k > 60 {
                break; // the original would overrun its path array forever
            }
        }
        c += 1;
    }
    m.set16(PATH + 4 * k, x);
    m.set16(PATH + 4 * k + 2, y);
    m.set8(WALKING, 1);
    m.set16(PATH + 4 * k + 4, -100);
    m.set16(PATH + 4 * k + 6, -100);
    m.set8(CUR, 0);
}

/// Abandon the current walk (click handler).
pub fn stop(m: &mut Memory) {
    m.set8(CUR, 0);
    m.set8(DIRX, 0);
    m.set8(DIRY, 0);
    m.set16(PATH, -100);
    m.set16(PATH + 2, -100);
}

/// PPOS(d) / door direction: pose the player.
pub fn face(m: &mut Memory, d: i32) {
    match d {
        0 => {
            m.set8(FRAME, m.u8(S_SIDE));
            m.set8(FLIP, 1);
        }
        1 => {
            m.set8(FRAME, m.u8(S_SIDE));
            m.set8(FLIP, 0);
        }
        2 => {
            m.set8(FRAME, m.u8(S_FRONT));
            m.set8(FLIP, 0);
        }
        3 => {
            m.set8(FRAME, m.u8(S_BACK));
            m.set8(FLIP, 0);
        }
        _ => {}
    }
}

/// The walker, once per main-loop pass.
pub fn tick(m: &mut Memory) {
    let p = |m: &Memory| PATH + 2 * m.u8(CUR) as usize;
    let px = |m: &Memory| m.i16(p(m));
    let py = |m: &Memory| m.i16(p(m) + 2);
    let pnx = |m: &Memory| m.i16(p(m) + 4);
    let pny = |m: &Memory| m.i16(p(m) + 6);
    let b = |m: &Memory, a: usize| m.u8(a);

    if px(m) != -100 || py(m) != -100 || b(m, SPEAKING) == 1 {
        if b(m, SPEAKING) == 0 && ((b(m, DIRX) == 0 && b(m, DIRY) == 0) || (b(m, DIRX) == 0xff && b(m, DIRY) == 0xff)) {
            if px(m) == -100 {
                m.set8(DIRX, 0xff);
            }
            if py(m) == -100 {
                m.set8(DIRY, 0xff);
            }
            if px(m) > feet_x(m) && b(m, DIRX) == 0 {
                m.set8(DIRX, 1);
            }
            if px(m) < feet_x(m) && b(m, DIRX) == 0 {
                m.set8(DIRX, 2);
            }
            if py(m) > feet_y(m) && b(m, DIRY) == 0 {
                m.set8(DIRY, 1);
            }
            if py(m) < feet_y(m) && b(m, DIRY) == 0 {
                m.set8(DIRY, 2);
            }
            // QUIRK: compares with the left edge, not the feet.
            if px(m) == m.i16(PX) {
                m.set8(DIRX, 0xff);
            }
            if py(m) == feet_y(m) {
                m.set8(DIRY, 0xff);
            }
            if b(m, DIRX) == 0 {
                m.set8(DIRX, 0xff);
            }
            if b(m, DIRY) == 0 {
                m.set8(DIRY, 0xff);
            }
            if b(m, DIRX) != 0xff && b(m, DIRY) != 0xff {
                let ddx = if b(m, DIRX) == 1 { px(m) - feet_x(m) } else { feet_x(m) - px(m) };
                let ddy = if b(m, DIRY) == 1 { py(m) - feet_y(m) } else { feet_y(m) - py(m) };
                if ddx <= 50 && ddy >= 10 {
                    m.set8(DIRX, b(m, DIRX) + 2);
                }
            }
        }
        if b(m, SPEAKING) != 0 {
            if b(m, FRAME) < b(m, S_TBACK) {
                if b(m, SAVED) == b(m, S_SIDE) {
                    m.set8(FRAME, b(m, S_TSIDE));
                    m.set8(ANIM, 0);
                }
                if b(m, SAVED) == b(m, S_BACK) {
                    m.set8(FRAME, b(m, S_TBACK));
                    m.set8(ANIM, 0);
                    m.set8(FLIP, 0);
                }
                if b(m, SAVED) == b(m, S_FRONT) {
                    m.set8(FRAME, b(m, S_TFRONT));
                    m.set8(ANIM, 0);
                    m.set8(FLIP, 0);
                }
            } else {
                m.set8(ANIM, 1);
            }
        }
        for d in [1, 2] {
            if b(m, DIRY) == d {
                let going = if d == 1 { py(m) > feet_y(m) } else { py(m) < feet_y(m) };
                if going {
                    m.set8(ANIM, 1);
                } else {
                    m.set8(DIRY, 0xff);
                    if pnx(m) == -100 && pny(m) == -100 {
                        let v = py(m) - m.i16(PH);
                        m.set16(PY, v);
                    }
                }
            }
        }
        let side_lo = |m: &Memory| b(m, FRAME) > b(m, S_SIDE) + b(m, C_SIDE) - 1 || b(m, FRAME) < b(m, S_SIDE) + 1;
        for (fwd, slow, flip) in [(1, 3, 0), (2, 4, 1)] {
            let dx = b(m, DIRX);
            if dx == fwd || dx == slow {
                let going = if fwd == 1 { px(m) > feet_x(m) } else { px(m) < feet_x(m) };
                if going {
                    m.set8(ANIM, 1);
                    if (dx == slow && m.i8(DIRY) <= 0) || dx == fwd {
                        if side_lo(m) {
                            m.set8(FRAME, b(m, S_SIDE) + 1);
                        }
                        m.set8(FLIP, flip);
                    }
                } else {
                    m.set8(DIRX, 0xff);
                }
            }
        }
    } else {
        m.set8(ANIM, 0);
        for (s, c) in [(S_SIDE, C_SIDE), (S_BACK, C_BACK), (S_FRONT, C_FRONT)] {
            let f = b(m, FRAME);
            if f > b(m, s) && b(m, s) + b(m, c) > f {
                m.set8(FRAME, b(m, s));
            }
        }
        m.set16(PATH, -100);
        m.set16(PATH + 2, -100);
        m.set8(WALKING, 0);
    }
    if b(m, DIRX) == 0xff && b(m, DIRY) == 0xff {
        m.set8(CUR, b(m, CUR) + 2);
        m.set8(DIRX, 0);
        m.set8(DIRY, 0);
    }
    if b(m, ANIM) == 1 && b(m, STEP_WAIT) == 0 {
        m.set8(ANIM, 0);
        m.set8(FRAME, b(m, FRAME) + 1);
        let f = |m: &Memory| b(m, FRAME);
        if f(m) == (b(m, S_SIDE) + b(m, C_SIDE) - 1) {
            m.set8(FRAME, b(m, S_SIDE) + 1);
        }
        // BUG (as v2): start + start instead of start + count.
        if f(m) == (b(m, S_BACK) + b(m, S_BACK) - 1) {
            m.set8(FRAME, b(m, S_BACK) + 1);
        }
        if f(m) == (b(m, S_FRONT) + b(m, S_FRONT) - 1) {
            m.set8(FRAME, b(m, S_FRONT) + 1);
        }
        if f(m) == (b(m, S_TSIDE) + b(m, C_TSIDE) - 1) {
            m.set8(FRAME, b(m, S_TSIDE));
        }
        if f(m) == (b(m, S_TBACK) + b(m, C_TBACK) - 1) {
            m.set8(FRAME, b(m, S_TBACK));
        }
        if f(m) == (b(m, S_TFRONT) + b(m, C_TFRONT) - 1) {
            m.set8(FRAME, b(m, S_TFRONT));
        }
        let (sx, sy) = (b(m, STEP_X), b(m, STEP_Y));
        let dy8 = m.i8(DIRY);
        let dx = b(m, DIRX);
        if dx == 1 || dx == 3 {
            if dx == 1 || dy8 <= 0 {
                m.set16(PX, m.i16(PX) + sx);
            }
            if dx == 3 && dy8 > 0 {
                m.set16(PX, m.i16(PX) + half(sx));
            }
            if px(m) <= feet_x(m) {
                let v = px(m) - half(m.i16(PW));
                m.set16(PX, v);
                if pnx(m) == -100 && pny(m) == -100 {
                    let v = px(m) - half(m.i16(PW));
                    m.set16(PX, v);
                }
            }
        }
        if dx == 2 || dx == 4 {
            if dx == 2 || dy8 <= 0 {
                m.set16(PX, m.i16(PX) - sx);
            }
            if dx == 4 && dy8 > 0 {
                m.set16(PX, m.i16(PX) - half(sx));
            }
            if px(m) >= feet_x(m) {
                let v = px(m) - half(m.i16(PW));
                m.set16(PX, v);
                if pnx(m) == -100 && pny(m) == -100 {
                    let v = px(m) - half(m.i16(PW));
                    m.set16(PX, v);
                }
            }
        }
        for (d, s, c) in [(1, S_FRONT, C_FRONT), (2, S_BACK, C_BACK)] {
            if b(m, DIRY) != d {
                continue;
            }
            let v = if b(m, DIRX) == 1 || b(m, DIRX) == 2 {
                half(sy)
            } else {
                if f(m) > b(m, s) + b(m, c) - 1 || f(m) < b(m, s) + 1 {
                    m.set8(FRAME, b(m, s) + 1);
                    m.set8(ANIM, 0);
                    m.set8(FLIP, 0);
                }
                sy
            };
            if d == 1 {
                m.set16(PY, m.i16(PY) + v);
                if py(m) <= feet_y(m) {
                    let v = py(m) - m.i16(PH);
                    m.set16(PY, v);
                }
            } else {
                m.set16(PY, m.i16(PY) - v);
                if py(m) >= feet_y(m) {
                    let v = py(m) - m.i16(PH);
                    m.set16(PY, v);
                }
            }
        }
        m.set8(STEP_WAIT, b(m, STEP_TICKS));
    }
    let fr = b(m, FRAME) as usize;
    m.set16(SRCX, m.i16(FRAMES + 4 * fr));
    m.set16(SRCY, m.i16(FRAMES + 4 * fr + 2));
    m.set16(PW, m.i16(FRAME_W));
    m.set16(PH, m.i16(FRAME_H));
}
