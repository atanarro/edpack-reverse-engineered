//! LIMITMOV rule generator of ED.EXE (0x93d9 generate, 0x9ae3 add, 0x9c9c sort).
//! Each LIMITMOV box becomes rules "if the target is in box T and the walker
//! is in box C, first go to waypoint W".  Bit-exact with the demo data.
//! rule = [used, tx1, ty1, tx2, ty2, cx1, cy1, cx2, cy2, wx, wy]

pub const RULES: usize = 320;
pub type Rule = [i32; 11];

pub fn new_rule_table() -> Vec<Rule> {
    vec![[0; 11]; RULES]
}

fn sort(t: &mut [Rule]) {
    for si in 0..RULES {
        if t[si][0] != 1 {
            continue;
        }
        for di in 0..RULES {
            if t[di][0] != 1 {
                continue;
            }
            let (a, b) = (t[di], t[si]);
            if a[3] == b[3] && a[4] == b[4] && (a[3] - a[1] > b[3] - b[1] || a[4] - a[2] > b[4] - b[2]) {
                t.swap(si, di);
            }
            let (a, b) = (t[di], t[si]);
            if a[1] == b[1] && a[2] == b[2] && (a[3] - a[1] < b[3] - b[1] || a[4] - a[2] < b[4] - b[2]) {
                t.swap(si, di);
            }
        }
    }
}

fn add(t: &mut [Rule], cur: [i32; 4], tgt: [i32; 4], wp: [i32; 2]) {
    let r: Rule = [1, tgt[0], tgt[1], tgt[2], tgt[3], cur[0], cur[1], cur[2], cur[3], wp[0], wp[1]];
    for e in t.iter().take_while(|e| e[0] == 1) {
        if e[1..] == r[1..] {
            return;
        }
    }
    if let Some(free) = t.iter().position(|e| e[0] == 0) {
        t[free] = r;
    }
    sort(t);
}

/// Add the rules for LIMITMOV(x1, y1, x2, y2).
pub fn add_limitmov(t: &mut [Rule], x1: i32, y1: i32, x2: i32, y2: i32) {
    let (lx, ty, rx, by) = (x1, y1, x2, y2);
    let (mut tl, mut tp, mut tr, mut l, mut r, mut bl, mut b, mut br) =
        (true, true, true, true, true, true, true, true);
    if x1 <= 0 {
        (tl, l, bl) = (false, false, false);
    }
    if y1 <= 0 {
        (tl, tp, tr) = (false, false, false);
    }
    if rx >= 319 {
        (tr, r, br) = (false, false, false);
    }
    if by >= 199 {
        (bl, b, br) = (false, false, false);
    }
    if tl {
        if r || br {
            add(t, [0, 0, rx, ty], [rx, ty, 320, 200], [rx, ty]);
        }
        if b || br {
            add(t, [0, 0, lx, by], [lx, by, 320, 200], [lx, by]);
        }
    }
    if tr {
        if b || bl {
            add(t, [rx, 0, 320, by], [0, by, rx, 200], [rx, by]);
        }
        if l || bl {
            add(t, [lx, 0, 320, ty], [0, ty, lx, 200], [lx, ty]);
        }
    }
    if bl {
        if r || tr {
            add(t, [0, by, rx, 200], [rx, 0, 320, by], [rx, by]);
        }
        if tp || tr {
            add(t, [0, ty, lx, 200], [lx, 0, 320, ty], [lx, ty]);
        }
    }
    if br {
        if tp || tl {
            add(t, [rx, ty, 320, 200], [0, 0, rx, ty], [rx, ty]);
        }
        if tl || l {
            add(t, [lx, by, 320, 200], [0, 0, lx, by], [lx, by]);
        }
    }
    if tp {
        if l || bl {
            add(t, [lx, 0, 320, ty], [0, ty, lx, 200], [lx, ty]);
        }
        if r || br {
            add(t, [0, 0, rx, ty], [rx, ty, 320, 200], [rx, ty]);
        }
        if b && l {
            add(t, [lx, 0, rx, ty], [lx, by, rx, 200], [lx, ty]);
            add(t, [0, ty, lx, by], [lx, by, rx, 200], [lx, by]);
        } else if b && r {
            add(t, [lx, 0, rx, ty], [lx, by, rx, 200], [rx, ty]);
            add(t, [rx, ty, 320, by], [lx, by, rx, 200], [rx, by]);
        }
    }
    if l {
        if tp || tr {
            add(t, [0, ty, lx, 200], [lx, 0, 320, ty], [lx, ty]);
        }
        if b || br {
            add(t, [0, 0, lx, by], [lx, by, 320, 200], [lx, by]);
        }
        if r && b {
            add(t, [0, ty, lx, by], [rx, ty, 320, by], [lx, by]);
            add(t, [lx, by, rx, 200], [rx, ty, 320, ty], [rx, by]);
        } else if r && tp {
            add(t, [0, ty, lx, by], [rx, ty, 320, by], [lx, ty]);
            add(t, [lx, 0, rx, ty], [rx, ty, 320, by], [rx, ty]);
        }
    }
    if r {
        if tl || tp {
            add(t, [rx, ty, 320, 200], [0, 0, rx, ty], [rx, ty]);
        }
        if bl || b {
            add(t, [rx, 0, 320, by], [0, by, rx, 200], [rx, by]);
        }
        if l && tp {
            add(t, [rx, ty, 320, by], [0, ty, lx, by], [rx, ty]);
            add(t, [lx, 0, rx, ty], [0, ty, lx, by], [lx, ty]);
        } else if l && b {
            add(t, [rx, ty, 320, by], [0, ty, lx, by], [rx, by]);
            add(t, [lx, by, rx, 200], [0, ty, lx, by], [lx, by]);
        }
    }
    if b {
        if tl || l {
            add(t, [lx, by, 320, 200], [0, 0, lx, by], [lx, by]);
        }
        if r || tr {
            add(t, [0, by, rx, 200], [rx, 0, 320, by], [rx, by]);
        }
        if tp && r {
            add(t, [lx, by, rx, 200], [lx, 0, rx, ty], [rx, by]);
            add(t, [rx, ty, 320, by], [lx, 0, rx, ty], [rx, ty]);
        } else if tp && l {
            add(t, [lx, by, rx, 200], [lx, 0, rx, ty], [lx, by]);
            add(t, [0, ty, lx, by], [lx, 0, rx, ty], [lx, ty]);
        }
    }
}
