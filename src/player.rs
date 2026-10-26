//! The player character: path planning (GOXY + LIMIT + LIMITMOV), the
//! per-tick walker, frame selection, scaling and shading.  Ports of
//! RUNDAT.EXE 0x7fdc (GOXY), 0x45d2 (walker), 0x429e (scaled blit), 0x685f
//! (SCALEMOV), 0x6930 (SHADOWMOV) and 0x5606 (layer swap).  All state lives
//! in emulated DS memory.

use crate::memory::{Memory, THING_SIZE, ds};

pub const VISIBLE: usize = 0xfc19;
pub const PREV_VISIBLE: usize = 0xfc17;
pub const FRAME: usize = 0xfbf5;
pub const PREV_FRAME: usize = 0xfbf6;
pub const SAVED_FRAME: usize = 0xfbf7;
pub const ANIMATE: usize = 0xfbf4;
pub const X: usize = 0xfc01;
pub const Y: usize = 0xfc03;
pub const SRCX: usize = 0xfc09;
pub const SRCY: usize = 0xfc0b;
pub const W: usize = 0xfc0d;
pub const H: usize = 0xfc0f;
pub const FLIP: usize = 0xfc1b;
pub const PMOV: usize = 0xfbf8;
pub const PMOV_X: usize = 0xfbf9;
pub const PMOV_Y: usize = 0xfbfb;
pub const PMOV_W: usize = 0xfbfd;
pub const PMOV_H: usize = 0xfbff;
pub const FEET_X: usize = 0xfd3c;
pub const FEET_Y: usize = 0xfd3e;
pub const RED_W: usize = 0xfcf2;
pub const RED_H: usize = 0xfcf4;
pub const SPEED_X: usize = 0xfcfa;
pub const SPEED_Y: usize = 0xfcfc;
pub const AREA_H: usize = 0xfd22;
pub const SHADOW_FIRST: usize = 0xfd27;
pub const SHADOW_COUNT: usize = 0xfd29;
pub const SHADE: usize = 0xfd25;
pub const SPEAKING: usize = 0xfcbf;
pub const WALKING: usize = 0xfcce;
pub const PATH_CUR: usize = 0xfc1f;
pub const DIR_X: usize = 0xfc1d;
pub const DIR_Y: usize = 0xfc1e;
pub const PATH: usize = 0xfc24;
pub const STEP_WAIT: usize = 0xfccf;
// NIVEL.MOV header bytes
pub const S_BACK: usize = 0xcf;
pub const S_FRONT: usize = 0xd0;
pub const S_SIDE: usize = 0xd1;
pub const S_TALK_SIDE: usize = 0xd2;
pub const S_TALK_BACK: usize = 0xd3;
pub const S_TALK_FRONT: usize = 0xd4;
pub const N_BACK: usize = 0xd6;
pub const N_FRONT: usize = 0xd7;
pub const N_SIDE: usize = 0xd8;
pub const N_TALK_BACK: usize = 0xd9;
pub const N_TALK_FRONT: usize = 0xda;
pub const N_TALK_SIDE: usize = 0xdb;
pub const MOV_W: usize = 0x4334;
pub const MOV_H: usize = 0x4336;
pub const POINTS: usize = 0x200c;
pub const STEP_TICKS: usize = 0x8a22;

const TERM: i32 = -100;

/// C integer division by two (truncates toward zero).
#[inline]
pub fn idiv2(v: i32) -> i32 {
    v / 2
}

pub fn feet(m: &Memory) -> (i32, i32) {
    (m.i16(X) + idiv2(m.i16(W)), m.i16(Y) + m.i16(H))
}

fn path(m: &Memory, k: usize) -> (i32, i32) {
    (m.i16(PATH + 4 * k), m.i16(PATH + 4 * k + 2))
}

/// The path array (30 points) is followed by other globals; overflow is
/// reproduced by writing through memory like the original.
fn set_path(m: &mut Memory, k: usize, x: i32, y: i32) {
    m.set16(PATH + 4 * k, x);
    m.set16(PATH + 4 * k + 2, y);
}

/// GOXY / click-to-walk path planner (0x7fdc).
pub fn go_xy(m: &mut Memory, x: i32, y: i32) {
    let (mut gx, mut gy, mut rawx, mut rawy) = (x, y, 0, 0);
    if gx >= 3000 {
        rawx = gx;
        gx -= 10000;
    }
    if gy >= 3000 {
        rawy = gy;
        gy -= 10000;
    }
    for i in 0..40 {
        let a = ds::LIMIT + i * 14;
        if m.i16(a) == 0 {
            continue;
        }
        let (x1, y1, x2, y2) = (m.i16(a + 2), m.i16(a + 4), m.i16(a + 6), m.i16(a + 8));
        let (tx, ty) = (m.i16(a + 10), m.i16(a + 12));
        if x1 <= gx && gx <= x2 && y1 <= gy && gy <= y2 {
            if tx != -100 && tx != -200 {
                gx = tx;
            }
            if tx == -100 {
                gx = m.i16(X) + idiv2(m.i16(W));
            }
            if ty != -100 && ty != -200 {
                gy = ty;
            }
            if ty == -100 {
                gy = m.i16(Y) + m.i16(H);
            }
        }
    }
    if rawx > 3000 {
        gx = rawx - 10000;
    }
    if rawy > 3000 {
        gy = rawy - 10000;
    }
    if m.i16(FEET_X) == gx && m.i16(FEET_Y) == gy {
        return;
    }

    let mut pass = 0;
    let mut k;
    let mut counts = [0, 0];
    loop {
        let (mut cx, mut cy) = (m.i16(FEET_X), m.i16(FEET_Y));
        k = 0;
        if pass < 2 {
            counts[pass] = 0;
        }
        let mut guard = 0;
        let mut c = 0usize;
        while c < 320 {
            let idx = if pass == 1 { c } else { 319 - c };
            let a = ds::LIMITMOV + idx * 22;
            if m.i16(a) == 1 {
                let r = |o: usize| m.i16(a + o);
                if r(2) <= gx
                    && gx <= r(6)
                    && r(4) <= gy
                    && gy <= r(8)
                    && r(10) <= cx
                    && cx <= r(14)
                    && r(12) <= cy
                    && cy <= r(16)
                {
                    let wx = if r(18) == TERM { cx } else { r(18) };
                    let wy = if r(20) == TERM { cy } else { r(20) };
                    if k < 64 {
                        set_path(m, k, wx, wy);
                    }
                    if wx != cx || wy != cy {
                        if pass < 2 {
                            counts[pass] += 1;
                        }
                        cx = wx;
                        cy = wy;
                        k += 1;
                        c = 0; // the increment below makes the rescan start at 1 (quirk)
                        guard += 1;
                        if guard > 64 {
                            break; // the original has no cycle guard and would hang
                        }
                    }
                }
            }
            c += 1;
        }
        if pass == 0 {
            pass = 1;
            continue;
        }
        // a longer first scan wins: it is repeated, since the second one overwrote the path
        if pass == 1 && counts[0] > counts[1] {
            pass = 2;
            continue;
        }
        break;
    }
    m.set8(PATH_CUR, 0);
    set_path(m, k, gx, gy);
    set_path(m, k + 1, TERM, TERM);
    m.set8(WALKING, 1);
}

/// Stop walking (click handler / SPEAK).
pub fn stop(m: &mut Memory) {
    m.set8(PATH_CUR, 0);
    m.set8(DIR_X, 0);
    m.set8(DIR_Y, 0);
    set_path(m, 0, TERM, TERM);
}

/// PPOS mapping (also used for GOTO / DOOR directions).
pub fn face(m: &mut Memory, dir: i32) {
    let (frame, flip) = match dir {
        0 => (m.u8(S_SIDE), 1),
        1 => (m.u8(S_SIDE), 0),
        2 => (m.u8(S_FRONT), 0),
        3 => (m.u8(S_BACK), 0),
        _ => return,
    };
    m.set8(FRAME, frame);
    m.set8(FLIP, flip);
}

/// Per-tick walker (0x45d2).
pub fn tick(m: &mut Memory) {
    let sb = |v: i32| v as u8 as i8 as i32;
    let (cf, d0, d1) = (m.u8(S_BACK), m.u8(S_FRONT), m.u8(S_SIDE));
    let (d2, d3, d4) = (m.u8(S_TALK_SIDE), m.u8(S_TALK_BACK), m.u8(S_TALK_FRONT));
    let (d6, d7, d8) = (m.u8(N_BACK), m.u8(N_FRONT), m.u8(N_SIDE));
    let (d9, da, db) = (m.u8(N_TALK_BACK), m.u8(N_TALK_FRONT), m.u8(N_TALK_SIDE));
    let cur = (m.u8(PATH_CUR) >> 1) as usize;
    let (px, py) = path(m, cur);
    let (nx, ny) = path(m, cur + 1);
    let fx = |m: &Memory| m.i16(X) + idiv2(m.i16(W));
    let fy = |m: &Memory| m.i16(Y) + m.i16(H);
    let (mut dir_x, mut dir_y) = (m.u8(DIR_X), m.u8(DIR_Y));
    let frame = |m: &Memory| m.u8(FRAME);
    let speaking = m.i16(SPEAKING);

    let idle = px == TERM && py == TERM && speaking != 1;
    if !idle {
        if speaking == 0 && ((dir_x == 0 && dir_y == 0) || (dir_x == 0xff && dir_y == 0xff)) {
            if px == TERM {
                dir_x = 0xff;
            }
            if py == TERM {
                dir_y = 0xff;
            }
            if px > fx(m) && dir_x == 0 {
                dir_x = 1;
            }
            if px < fx(m) && dir_x == 0 {
                dir_x = 2;
            }
            if py > fy(m) && dir_y == 0 {
                dir_y = 1;
            }
            if py < fy(m) && dir_y == 0 {
                dir_y = 2;
            }
            if px == fx(m) {
                dir_x = 0xff;
            }
            if py == fy(m) {
                dir_y = 0xff;
            }
            if dir_x == 0 {
                dir_x = 0xff;
            }
            if dir_y == 0 {
                dir_y = 0xff;
            }
            if dir_x != 0xff && dir_y != 0xff && (px - fx(m)).abs() <= 50 && (py - fy(m)).abs() >= 10 {
                dir_x += 2;
            }
        }
        if speaking != 0 {
            if frame(m) < d3 {
                let saved = m.u8(SAVED_FRAME);
                if saved == d1 {
                    m.set8(FRAME, d2);
                    m.set8(ANIMATE, 0);
                }
                if saved == cf {
                    m.set8(FRAME, d3);
                    m.set8(ANIMATE, 0);
                    m.set8(FLIP, 0);
                }
                if saved == d0 {
                    m.set8(FRAME, d4);
                    m.set8(ANIMATE, 0);
                    m.set8(FLIP, 0);
                }
            } else {
                m.set8(ANIMATE, 1);
            }
        }
        if dir_y == 1 {
            if py > fy(m) {
                m.set8(ANIMATE, 1);
            } else {
                dir_y = 0xff;
                if nx == TERM && ny == TERM {
                    let h = m.i16(H);
                    m.set16(Y, py - h);
                }
            }
        }
        if dir_y == 2 {
            if py < fy(m) {
                m.set8(ANIMATE, 1);
            } else {
                dir_y = 0xff;
                if nx == TERM && ny == TERM {
                    let h = m.i16(H);
                    m.set16(Y, py - h);
                }
            }
        }
        if dir_x == 1 || dir_x == 3 {
            if px > fx(m) {
                m.set8(ANIMATE, 1);
                if dir_x == 1 || (dir_x == 3 && sb(dir_y) <= 0) {
                    if frame(m) > d1 + d8 - 1 || frame(m) < d1 + 1 {
                        m.set8(FRAME, d1 + 1);
                    }
                    m.set8(FLIP, 0);
                }
            } else {
                dir_x = 0xff;
            }
        }
        if dir_x == 2 || dir_x == 4 {
            if px < fx(m) {
                m.set8(ANIMATE, 1);
                if dir_x == 2 || (dir_x == 4 && sb(dir_y) <= 0) {
                    if frame(m) > d1 + d8 - 1 || frame(m) < d1 + 1 {
                        m.set8(FRAME, d1 + 1);
                    }
                    m.set8(FLIP, 1);
                }
            } else {
                dir_x = 0xff;
            }
        }
    } else {
        m.set8(ANIMATE, 0);
        let f = frame(m);
        if d1 < f && f < d1 + d8 {
            m.set8(FRAME, d1);
        }
        if cf < f && f < cf + d6 {
            m.set8(FRAME, cf);
        }
        if d0 < f && f < d0 + d7 {
            m.set8(FRAME, d0);
        }
        set_path(m, 0, TERM, TERM);
        m.set8(WALKING, 0);
    }
    if dir_x == 0xff && dir_y == 0xff {
        let c = m.u8(PATH_CUR);
        m.set8(PATH_CUR, c + 2);
        dir_x = 0;
        dir_y = 0;
    }
    m.set8(DIR_X, dir_x);
    m.set8(DIR_Y, dir_y);

    if m.u8(ANIMATE) == 1 && m.u8(STEP_WAIT) == 0 {
        m.set8(ANIMATE, 0);
        let f = frame(m) + 1;
        m.set8(FRAME, f);
        let check = |m: &mut Memory, at: i32, to: i32| {
            if frame(m) == (at & 0xff) {
                m.set8(FRAME, to);
            }
        };
        check(m, d1 + d8 - 1, d1 + 1);
        check(m, cf + cf - 1, cf + 1); // original bug (cf twice)
        check(m, d0 + d0 - 1, d0 + 1); // original bug (d0 twice)
        check(m, d2 + db - 1, d2);
        check(m, d3 + d9 - 1, d3);
        check(m, d4 + da - 1, d4);
        let (sx, sy) = (m.i16(SPEED_X), m.i16(SPEED_Y));
        if dir_x == 1 || dir_x == 3 {
            if dir_x == 1 || (dir_x == 3 && sb(dir_y) <= 0) {
                let v = m.i16(X) + sx;
                m.set16(X, v);
            }
            if dir_x == 3 && sb(dir_y) > 0 {
                let v = m.i16(X) + idiv2(sx);
                m.set16(X, v);
            }
            if px <= fx(m) {
                let v = px - idiv2(m.i16(W));
                m.set16(X, v);
            }
        }
        if dir_x == 2 || dir_x == 4 {
            if dir_x == 2 || (dir_x == 4 && sb(dir_y) <= 0) {
                let v = m.i16(X) - sx;
                m.set16(X, v);
            }
            if dir_x == 4 && sb(dir_y) > 0 {
                let v = m.i16(X) - idiv2(sx);
                m.set16(X, v);
            }
            if px >= fx(m) {
                let v = px - idiv2(m.i16(W));
                m.set16(X, v);
            }
        }
        if dir_y == 1 {
            let v = if dir_x == 1 || dir_x == 2 {
                idiv2(sy)
            } else {
                if frame(m) > d0 + d7 - 1 || frame(m) < d0 + 1 {
                    m.set8(FRAME, d0 + 1);
                    m.set8(ANIMATE, 0);
                    m.set8(FLIP, 0);
                }
                sy
            };
            let ny = m.i16(Y) + v;
            m.set16(Y, ny);
            if py <= fy(m) {
                let h = m.i16(H);
                m.set16(Y, py - h);
            }
        }
        if dir_y == 2 {
            let v = if dir_x == 1 || dir_x == 2 {
                idiv2(sy)
            } else {
                if frame(m) > cf + d6 - 1 || frame(m) < cf + 1 {
                    m.set8(FRAME, cf + 1);
                    m.set8(ANIMATE, 0);
                    m.set8(FLIP, 0);
                }
                sy
            };
            let ny = m.i16(Y) - v;
            m.set16(Y, ny);
            if py >= fy(m) {
                let h = m.i16(H);
                m.set16(Y, py - h);
            }
        }
        let st = m.u8(STEP_TICKS);
        m.set8(STEP_WAIT, if speaking == 0 { st } else { 2 * st });
    }
    update_source(m);
}

/// Current frame source rectangle and size (end of the walker tick).
pub fn update_source(m: &mut Memory) {
    if m.u8(PMOV) == 0 {
        let f = m.u8(FRAME) as usize;
        let (sx, sy) = (m.i16(POINTS + f * 4), m.i16(POINTS + f * 4 + 2));
        m.set16(SRCX, sx);
        m.set16(SRCY, sy);
        let (w, h) = (m.i16(MOV_W), m.i16(MOV_H));
        m.set16(W, w);
        m.set16(H, h);
    } else {
        let v = [m.i16(PMOV_X), m.i16(PMOV_Y), m.i16(PMOV_W), m.i16(PMOV_H)];
        m.set16(SRCX, v[0]);
        m.set16(SRCY, v[1]);
        m.set16(W, v[2]);
        m.set16(H, v[3]);
    }
    let (x, y) = feet(m);
    m.set16(FEET_X, x);
    m.set16(FEET_Y, y);
}

/// SCALEMOV (0x685f).
pub fn apply_scale_mov(m: &mut Memory) {
    let (x, y) = feet(m);
    for i in 0..20 {
        let a = ds::SCALEMOV + i * 18;
        if m.i16(a) != 1 {
            break;
        }
        if m.i16(a + 2) <= x && x <= m.i16(a + 6) && m.i16(a + 4) <= y && y <= m.i16(a + 8) {
            let v = [m.i16(a + 10), m.i16(a + 12), m.i16(a + 14), m.i16(a + 16)];
            m.set16(RED_W, v[0]);
            m.set16(RED_H, v[1]);
            if v[2] != -100 {
                m.set16(SPEED_X, v[2]);
            }
            if v[3] != -100 {
                m.set16(SPEED_Y, v[3]);
            }
            break;
        }
    }
}

/// SHADOWMOV (0x6930): shade the palette range when the player's feet enter
/// a SHADOWMOV zone with a new shade.
pub fn apply_shadow(m: &mut Memory) {
    let Some(shade) = shadow_mov(m) else { return };
    let (first, count) = (m.i16(SHADOW_FIRST), m.i16(SHADOW_COUNT));
    if (0..256).contains(&first) && count > 0 && first + count < 256 {
        crate::engine::mod_pal(m, first, count, shade);
    }
    m.set8(crate::engine::g::PAL_DIRTY, 1);
}

/// The SHADOWMOV zone lookup of [`apply_shadow`]: the new shade, if it changed.
pub fn shadow_mov(m: &mut Memory) -> Option<i32> {
    let (x, y) = feet(m);
    for i in 0..20 {
        let a = ds::SHADOWMOV + i * 12;
        if m.i16(a) != 1 {
            break;
        }
        if m.i16(a + 2) <= x && x <= m.i16(a + 6) && m.i16(a + 4) <= y && y <= m.i16(a + 8) {
            let shade = m.i16(a + 10);
            if shade != m.i16(SHADE) {
                m.set16(SHADE, shade);
                return Some(shade);
            }
            return None;
        }
    }
    None
}

/// Layer auto-swap of things with layer 0/1 (0x5606).  Mode and layer are
/// read and written as words here, so the byte after each takes part.
pub fn layer_swap(m: &mut Memory) {
    let pf = (m.i16(Y) + m.i16(MOV_H)) as i16;
    for t in 0..30 {
        let a = ds::THINGS + t * THING_SIZE;
        if m.u16(a + 0x2b) == 0 {
            continue;
        }
        let layer = m.u16(a + 0x15);
        if layer == 2 || layer == 3 {
            continue;
        }
        let front = (m.i16(a + 0x1b) + m.i16(a + 0x23)) as i16 >= pf;
        m.set16(a + 0x15, front as i32);
    }
}

/// Scaled, optionally mirrored blit of the player (0x429e).
pub fn draw(m: &Memory, dst: &mut [u8], src: &[u8]) {
    let (w, h) = (m.i16(W), m.i16(H));
    let (red_w, red_h) = (m.i16(RED_W), m.i16(RED_H));
    let dstx = m.i16(X) + idiv2(red_w);
    let dsty = m.i16(Y) + red_h;
    blit_scaled(dst, src, m.i16(SRCX), m.i16(SRCY), dstx, dsty, w, h, red_w, red_h, m.u8(FLIP), m.i16(AREA_H));
}

/// 0x429e: nearest-neighbour row/column dropping with float32 steps.
#[allow(clippy::too_many_arguments)]
pub fn blit_scaled(
    dst: &mut [u8],
    src: &[u8],
    srcx: i32,
    srcy: i32,
    dstx: i32,
    dsty: i32,
    w: i32,
    h: i32,
    mut red_w: i32,
    mut red_h: i32,
    flip: i32,
    clip_y: i32,
) {
    if w <= 0 || h <= 0 {
        return;
    }
    if h <= red_h {
        red_h = h - 1;
    }
    red_h = red_h.max(0);
    if w <= red_w {
        red_w = w - 1;
    }
    red_w = red_w.max(0);
    let kh = ((h - red_h) as f64 / h as f64) as f32;
    let kw = ((w - red_w) as f64 / w as f64) as f32;
    let (mut fy, mut last_row) = (0f32, -1i32);
    for sr in 0..h {
        let r = fy.trunc() as i32;
        if r != last_row {
            let (mut fx, mut last_col) = (0f32, -1i32);
            for c in (if flip == 1 { 1 } else { 0 })..w {
                let d = fx.trunc() as i32;
                if d != last_col {
                    let sx = if flip == 1 { srcx + w - 1 - c } else { srcx + c };
                    let sy = srcy + sr;
                    if (0..320).contains(&sx) && (0..200).contains(&sy) {
                        let p = src[(sy * 320 + sx) as usize];
                        let (yy, xx) = (dsty + r, dstx + d);
                        let below = if flip == 1 { yy < clip_y } else { yy <= clip_y };
                        if p != 0 && yy >= 0 && below && yy < 200 && (0..320).contains(&xx) {
                            dst[(yy * 320 + xx) as usize] = p;
                        }
                    }
                    last_col = d;
                }
                fx += kw;
            }
            last_row = r;
        }
        fy += kh;
    }
}
