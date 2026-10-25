//! A modern ED.EXE: compiles EDPACK sources (.ED) into the compiled NIVEL.*
//! files understood by RUNFILES.EXE and by this engine (loose-file mode).
//!
//! ```text
//! .LEVEL n   -> NIVEL.nnn     .INVENTARY -> NIVEL.INV   .MOVS  -> NIVEL.MOV
//! .MOUSE     -> NIVEL.MOU     .ACINVINV  -> NIVEL.IAC   .TITLE -> NIVEL.TIT
//! .LEVEL n + .ONLYxxx + .DATFILE"name" -> a LOADDATA data file
//! ```
//! Unlike the original every file starts from a clean state (ED.EXE leaks
//! data from the previously compiled file into unused tables).

use std::collections::BTreeMap;

use super::codegen::compile_script;
use super::limitmov::{RULES, add_limitmov, new_rule_table};
use super::parser::{Arg, Item, Stmt, parse_ed};
use crate::Error;
use crate::pcx::decode_pcx;

fn header(kind: u8) -> Vec<u8> {
    let mut h = vec![0u8; 30];
    h[0] = 2;
    h[3] = kind;
    h
}

/// RLE writer (0x1667): runs of 2..63, or single bytes >= 0xC0, as (0xC0|n, b).
pub fn rle_encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let mut n = 1;
        while i + n < bytes.len() && bytes[i + n] == b && n < 63 {
            n += 1;
        }
        if n >= 2 || b >= 0xc0 {
            out.extend([0xc0 | n as u8, b]);
            i += n;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

struct Buf(Vec<u8>);

impl Buf {
    fn new(size: usize) -> Self {
        Buf(vec![0; size])
    }
    fn w16(&mut self, o: usize, x: i32) {
        self.0[o..o + 2].copy_from_slice(&(x as i16).to_le_bytes());
    }
    fn w8(&mut self, o: usize, x: i32) {
        self.0[o] = x as u8;
    }
    fn str(&mut self, o: usize, s: &[u8], key: u8, max: usize) {
        for (i, &c) in s.iter().take(max - 1).enumerate() {
            self.0[o + i] = c.wrapping_add(key);
        }
    }
}

fn num(a: Option<&Arg>, ctx: &str) -> Result<i32, Error> {
    match a {
        Some(Arg::Num(n)) => Ok(*n),
        _ => Err(Error::new(format!("{ctx}: se esperaba un número"))),
    }
}

fn string<'a>(a: Option<&'a Arg>, ctx: &str) -> Result<&'a [u8], Error> {
    match a {
        Some(Arg::Str(s)) => Ok(s),
        _ => Err(Error::new(format!("{ctx}: se esperaba un texto entre comillas"))),
    }
}

fn index(v: i32, max: usize, ctx: &str) -> Result<usize, Error> {
    if v >= 0 && (v as usize) < max {
        Ok(v as usize)
    } else {
        Err(Error::new(format!("{ctx}: número fuera de rango")))
    }
}

/// Result of compiling one source file: file name -> bytes.
fn compile_level(items: &[Item], file: &str, level: i32) -> Result<(String, Vec<u8>), Error> {
    let (mut gfx, mut files, mut limit, mut things) =
        (Buf::new(0x34), Buf::new(0x104), Buf::new(0x230), Buf::new(0xac8));
    let mut order = vec![0xffu8; 0x1e];
    let (mut doors, mut pool, mut offs, mut code) =
        (Buf::new(0xc8), vec![0u8; 0x61a8], Buf::new(0x1f4), Buf::new(0x2710));
    let (mut actions, mut anims, mut acinvth) = (Buf::new(0x64), Buf::new(0x1f4), Buf::new(0xf0));
    let (mut cooranim, mut selectext) = (Buf::new(0x3e8), Buf::new(0x258));
    let mut msgs: Vec<Buf> = (0..5).map(|_| Buf::new(250)).collect();
    let (mut scalemov, mut shadowmov, mut deframes) = (Buf::new(0x168), Buf::new(0xf0), Buf::new(0xa28));
    let mut rules = new_rule_table();
    for i in 0..250 {
        offs.w16(i * 2, -1);
    }
    let (mut pool_pos, mut code_pos) = (0usize, 1usize);
    let (mut n_limit, mut n_acinvth, mut n_scale, mut n_shadow) = (0, 0, 0, 0);
    let (mut only, mut datfile): (Option<String>, Option<Vec<u8>>) = (None, None);

    let mut emit_script = |body: &[Stmt], code: &mut Buf| -> Result<usize, Error> {
        let words = compile_script(body, file)?;
        if code_pos + words.len() > 5000 {
            return Err(Error::new(format!("{file}: demasiado código en el nivel")));
        }
        let base = code_pos;
        for (i, w) in words.iter().enumerate() {
            code.w16((base + i) * 2, *w);
        }
        code_pos += words.len();
        Ok(base)
    };

    for it in items {
        match it {
            Item::Directive { name, arg, .. } => {
                if let Some(k) = name.strip_prefix("ONLY") {
                    only = Some(k.to_string());
                }
                if name == "DATFILE"
                    && let Some(Arg::Str(s)) = arg
                {
                    datfile = Some(s.clone());
                }
            }
            Item::Block { kind, n, body, line, .. } => {
                let ctx = format!("{file}:{line} {kind}");
                match kind.as_str() {
                    "ACTION" => {
                        let base = emit_script(body, &mut code)?;
                        actions.w16(index(*n, 50, &ctx)? * 2, base as i32);
                    }
                    "ANIMATION" => {
                        let base = emit_script(body, &mut code)?;
                        let a = index(*n, 50, &ctx)? * 10;
                        anims.w16(a, 0);
                        anims.w16(a + 2, base as i32);
                    }
                    "THING" => compile_thing(index(*n, 30, &ctx)?, body, &mut things, &mut order, file)?,
                    _ => return Err(Error::new(format!("{ctx}: bloque inesperado en un nivel"))),
                }
            }
            Item::Stmt(Stmt::Cmd { name, args, line }) => {
                let ctx = format!("{file}:{line} {name}");
                let a = |i: usize| args.get(i);
                match name.as_str() {
                    "ESCENARIOPCX" => gfx.str(0, string(a(0), &ctx)?, 0x2d, 13),
                    "DIBUJOSPCX" => gfx.str(13, string(a(0), &ctx)?, 0x2d, 13),
                    "MOVSPCX" => gfx.str(26, string(a(0), &ctx)?, 0x2d, 13),
                    "FILE" => files.str(index(num(a(0), &ctx)?, 20, &ctx)? * 13, string(a(1), &ctx)?, 0x19, 13),
                    "LIMIT" => {
                        if n_limit >= 40 {
                            return Err(Error::new(format!("{ctx}: demasiados LIMIT")));
                        }
                        let o = n_limit * 14;
                        n_limit += 1;
                        limit.w16(o, 1);
                        for k in 0..6 {
                            limit.w16(o + 2 + k * 2, num(a(k), &ctx)?);
                        }
                    }
                    "LIMITMOV" => {
                        add_limitmov(&mut rules, num(a(0), &ctx)?, num(a(1), &ctx)?, num(a(2), &ctx)?, num(a(3), &ctx)?)
                    }
                    "DOOR" => {
                        let o = index(num(a(0), &ctx)?, 20, &ctx)? * 10;
                        doors.w16(o, 1);
                        for k in 1..5 {
                            doors.w16(o + k * 2, num(a(k), &ctx)?);
                        }
                    }
                    "TEXT" => {
                        let id = index(num(a(0), &ctx)?, 250, &ctx)?;
                        let s = string(a(1), &ctx)?;
                        if pool_pos + s.len() + 1 > pool.len() {
                            return Err(Error::new(format!("{ctx}: no caben más textos")));
                        }
                        offs.w16(id * 2, pool_pos as i32);
                        pool[pool_pos..pool_pos + s.len()].copy_from_slice(s);
                        pool_pos += s.len() + 1;
                    }
                    "ACINVTH" => {
                        if n_acinvth >= 30 {
                            return Err(Error::new(format!("{ctx}: demasiados ACINVTH")));
                        }
                        let o = n_acinvth * 8;
                        n_acinvth += 1;
                        for k in 0..4 {
                            acinvth.w16(o + k * 2, num(a(k), &ctx)?);
                        }
                    }
                    "COORANIM" => {
                        let o = index(num(a(0), &ctx)?, 100, &ctx)? * 10;
                        for k in 0..4 {
                            cooranim.w16(o + k * 2, num(a(k + 1), &ctx)?);
                        }
                        cooranim.w16(o + 8, 1);
                    }
                    "SELECTEXT" => {
                        let o = index(num(a(0), &ctx)?, 50, &ctx)? * 12;
                        for k in 0..5 {
                            selectext.w16(o + k * 2, num(a(k + 1), &ctx)?);
                        }
                        selectext.w16(o + 10, 1);
                    }
                    "DEFFRAMES" => {
                        let o = index(num(a(0), &ctx)?, 50, &ctx)? * 52;
                        let mut k = 0;
                        for x in &args[1..] {
                            let v = num(Some(x), &ctx)?;
                            if v == -1 || k >= 50 {
                                break;
                            }
                            deframes.w8(o + 1 + k, v);
                            k += 1;
                        }
                        deframes.w8(o + 1 + k, 0xff);
                    }
                    "SCALEMOV" => {
                        if n_scale >= 20 {
                            return Err(Error::new(format!("{ctx}: demasiados SCALEMOV")));
                        }
                        let o = n_scale * 18;
                        n_scale += 1;
                        scalemov.w16(o, 1);
                        for k in 0..8 {
                            scalemov.w16(o + 2 + k * 2, num(a(k), &ctx)?);
                        }
                    }
                    "SHADOWMOV" => {
                        if n_shadow >= 20 {
                            return Err(Error::new(format!("{ctx}: demasiados SHADOWMOV")));
                        }
                        let o = n_shadow * 12;
                        n_shadow += 1;
                        shadowmov.w16(o, 1);
                        for k in 0..5 {
                            shadowmov.w16(o + 2 + k * 2, num(a(k), &ctx)?);
                        }
                    }
                    "NOACINVINV" => msgs[1].str(0, string(a(0), &ctx)?, 0x0f, 250),
                    "NOACINVOBJ" => msgs[2].str(0, string(a(0), &ctx)?, 0x0f, 250),
                    "YESACINVINV" => msgs[4].str(0, string(a(0), &ctx)?, 0x0f, 250),
                    _ => return Err(Error::new(format!("{ctx}: definición desconocida"))),
                }
            }
            Item::Stmt(Stmt::Nop) => {}
            Item::Stmt(_) => return Err(Error::new(format!("{file}: instrucción fuera de un ACTION/ANIMATION"))),
        }
    }
    let mut limitmov = Buf::new(RULES * 22);
    for (i, r) in rules.iter().enumerate() {
        for (k, v) in r.iter().enumerate() {
            limitmov.w16(i * 22 + k * 2, *v);
        }
    }
    for b in pool.iter_mut() {
        *b = b.wrapping_add(0x0f);
    }

    if let Some(kind) = only {
        let (k, data) = match kind.as_str() {
            "LIMIT" => (0, &limit.0),
            "LIMITMOV" => (1, &limitmov.0),
            "SHADOWMOV" => (2, &shadowmov.0),
            "SCALEMOV" => (3, &scalemov.0),
            "SELECTEXT" => (4, &selectext.0),
            "COORANIM" => (5, &cooranim.0),
            "DEFFRAMES" => (6, &deframes.0),
            _ => return Err(Error::new(format!("{file}: orden .ONLY{kind} desconocida"))),
        };
        let name = datfile.ok_or_else(|| Error::new(format!("{file}: .ONLY{kind} necesita .DATFILE\"nombre\"")))?;
        let mut out = header(10 + k);
        out.extend(rle_encode(data));
        return Ok((String::from_utf8_lossy(&name).to_uppercase(), out));
    }

    let mut out = header(0);
    let blocks: Vec<&[u8]> = vec![
        &gfx.0,
        &[],
        &files.0,
        &limitmov.0,
        &[],
        &limit.0,
        &things.0,
        &order,
        &[],
        &doors.0,
        &pool,
        &[],
        &offs.0,
        &code.0,
        &actions.0,
        &anims.0,
        &[],
        &acinvth.0,
        &cooranim.0,
        &selectext.0,
        &[],
    ];
    for b in blocks {
        if b.is_empty() { out.push(0) } else { out.extend(rle_encode(b)) }
    }
    for m in &msgs {
        out.extend(rle_encode(&m.0));
    }
    for b in [&scalemov.0, &shadowmov.0, &deframes.0] {
        out.extend(rle_encode(b));
    }
    Ok((format!("NIVEL.{level:03}"), out))
}

fn compile_thing(n: usize, body: &[Stmt], things: &mut Buf, order: &mut [u8], file: &str) -> Result<(), Error> {
    let o = n * 92;
    things.w8(o + 8, 1);
    for s in body {
        let Stmt::Cmd { name, args, line } = s else {
            if matches!(s, Stmt::Nop) {
                continue;
            }
            return Err(Error::new(format!("{file}: instrucción inesperada en un THING")));
        };
        let ctx = format!("{file}:{line} {name}");
        let n_at = |i: usize| num(args.get(i), &ctx);
        match name.as_str() {
            "NAME" => {
                for (i, &c) in string(args.first(), &ctx)?.iter().take(14).enumerate() {
                    things.w8(o + 0x31 + i, c as i32 + 0x0d);
                }
            }
            "INDEX" => {
                let i = n_at(0)?;
                if (0..30).contains(&i) {
                    order[i as usize] = n as u8;
                }
            }
            "AREA" => {
                for k in 0..4 {
                    things.w16(o + k * 2, n_at(k)?);
                }
            }
            "COOR" => {
                things.w16(o + 9, n_at(0)?);
                things.w16(o + 0x0b, n_at(1)?);
                things.w16(o + 0x21, n_at(2)?);
                things.w16(o + 0x23, n_at(3)?);
            }
            "POS" => {
                things.w16(o + 0x19, n_at(0)?);
                things.w16(o + 0x1b, n_at(1)?);
                things.w8(o + 0x2d, n_at(2)?);
                things.w8(o + 0x2b, n_at(3)?);
                things.w8(o + 0x15, n_at(4)?);
            }
            "GOTO" => {
                things.w16(o + 0x40, n_at(0)?);
                things.w16(o + 0x42, n_at(1)?);
                things.w16(o + 0x44, n_at(2)?);
            }
            "ACTIONS" => {
                for k in 0..5 {
                    things.w16(o + 0x48 + k * 2, n_at(k)?);
                }
            }
            _ => return Err(Error::new(format!("{ctx}: orden de THING desconocida"))),
        }
    }
    Ok(())
}

/// .INVENTARY: objects with name, description and a 35x26 picture cut from a PCX.
fn compile_inventory(
    items: &[Item],
    file: &str,
    resources: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let mut records = Vec::new();
    let mut palette = vec![0u8; 768];
    for it in items {
        let Item::Block { kind, body, .. } = it else { continue };
        if kind != "OBJINV" {
            continue;
        }
        let mut rec = Buf::new(0x4aa);
        for s in body {
            let Stmt::Cmd { name, args, line } = s else { continue };
            let ctx = format!("{file}:{line} {name}");
            match name.as_str() {
                "NAME" => rec.str(0xfa, string(args.first(), &ctx)?, 0x20, 30),
                "TEXT" => rec.str(0, string(args.first(), &ctx)?, 0x1e, 250),
                "PCX" => {
                    let pcx_name = String::from_utf8_lossy(string(args.first(), &ctx)?).to_string();
                    let bytes =
                        resources(&pcx_name).ok_or_else(|| Error::new(format!("{ctx}: no encuentro {pcx_name}")))?;
                    let img = decode_pcx(&bytes)?;
                    palette = img.palette.clone();
                    let (x0, y0) = (num(args.get(1), &ctx)?, num(args.get(2), &ctx)?);
                    rec.w16(0x118, 35);
                    rec.w16(0x11a, 26);
                    for y in 0..26 {
                        for x in 0..35 {
                            let (px, py) = (x0 + x, y0 + y);
                            let v = if (0..320).contains(&px)
                                && (0..200).contains(&py)
                                && (py * 320 + px) < img.pixels.len() as i32
                            {
                                img.pixels[(py * 320 + px) as usize]
                            } else {
                                0
                            };
                            rec.0[0x11c + (y * 35 + x) as usize] = v;
                        }
                    }
                }
                _ => return Err(Error::new(format!("{ctx}: orden de OBJINV desconocida"))),
            }
        }
        records.push(rec.0);
    }
    let mut out = header(1);
    out.extend((records.len() as u16).to_le_bytes());
    out.extend(&palette);
    for r in records {
        out.extend(r);
    }
    Ok(out)
}

fn compile_movs(items: &[Item]) -> Vec<u8> {
    let mut entries = Vec::new();
    for it in items {
        let Item::Block { kind, values: v, .. } = it else { continue };
        if kind != "DEFMOV" {
            continue;
        }
        let mut e = Buf::new(0x1a0);
        e.w16(0, v.first().copied().unwrap_or(0));
        e.w16(2, v.get(1).copied().unwrap_or(0));
        let mut pts: Vec<(i32, i32)> = v[2.min(v.len())..].as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect();
        if pts.last().is_none_or(|p| p.0 != -100) {
            pts.push((-100, -100));
        }
        let (mut starts, mut frames, mut n) = (vec![0usize], Vec::new(), 0);
        for (i, &(x, _)) in pts.iter().enumerate() {
            if x == -100 {
                frames.push(n);
                n = 0;
                if i + 1 < pts.len() {
                    starts.push(i + 1);
                }
            } else {
                n += 1;
            }
        }
        // header order: start back, front, side, talk back, talk front, talk side; then counts
        for k in 0..6 {
            e.w8(4 + k, starts.get(k).copied().unwrap_or(0) as i32);
        }
        for k in 0..6 {
            let f = frames.get(k).copied().unwrap_or(0);
            e.w8(10 + k, if k == 0 { f } else { f + 1 });
        }
        for i in 0..100 {
            let p = pts.get(i).copied().unwrap_or((-1, -1));
            e.w16(16 + i * 4, p.0);
            e.w16(18 + i * 4, p.1);
        }
        entries.push(e.0);
    }
    let mut out = header(2);
    out.extend((entries.len() as u16).to_le_bytes());
    for e in entries {
        out.extend(e);
    }
    out
}

fn compile_mouse(items: &[Item]) -> Vec<u8> {
    let mut recs = Vec::new();
    for it in items {
        let Item::Block { kind, values, .. } = it else { continue };
        if kind != "DEFMOUSE" {
            continue;
        }
        let mut e = Buf::new(72);
        for (i, v) in values.iter().take(36).enumerate() {
            e.w16(i * 2, *v);
        }
        recs.push(e.0);
    }
    let mut out = header(3);
    out.extend((recs.len() as u16).to_le_bytes());
    for r in recs {
        out.extend(r);
    }
    out
}

fn compile_iac(items: &[Item], file: &str) -> Result<Vec<u8>, Error> {
    let mut t = Buf::new(0x1f4);
    let mut n = 0;
    for it in items {
        let Item::Stmt(Stmt::Cmd { name, args, line }) = it else { continue };
        if name != "ACINVINV" {
            continue;
        }
        if n >= 50 {
            return Err(Error::new(format!("{file}: demasiados ACINVINV")));
        }
        let ctx = format!("{file}:{line}");
        let o = n * 10;
        n += 1;
        t.w16(o, 1);
        for k in 0..4 {
            t.w16(o + 2 + k * 2, num(args.get(k), &ctx)?);
        }
    }
    let mut out = header(4);
    out.extend(t.0);
    Ok(out)
}

fn compile_title(items: &[Item], file: &str) -> Result<Vec<u8>, Error> {
    let mut title: &[u8] = &[];
    for it in items {
        if let Item::Stmt(Stmt::Cmd { name, args, line }) = it
            && name == "TITLE"
        {
            title = string(args.first(), &format!("{file}:{line}"))?;
        }
    }
    let mut b = Buf::new(0x34);
    let mut sum = 0i32;
    for (i, &c) in title.iter().take(49).enumerate() {
        b.w8(i, c as i32 - 0xa5);
        sum += c as i32;
    }
    b.w16(0x32, sum);
    let mut out = header(5);
    out.extend(b.0);
    Ok(out)
}

/// Compile a set of source files (name -> source bytes in code page 850).
/// `resources` looks up PCX files (needed by .INVENTARY).
pub fn compile_project(
    sources: &BTreeMap<String, Vec<u8>>,
    resources: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut out = BTreeMap::new();
    for (file, src) in sources {
        let items = parse_ed(src, file)?;
        let dir = items.iter().find_map(|i| match i {
            Item::Directive { name, arg, .. } if !name.starts_with("ONLY") && name != "DATFILE" => {
                Some((name.clone(), arg.clone()))
            }
            _ => None,
        });
        let Some((name, arg)) = dir else {
            return Err(Error::new(format!("{file}: falta la orden para el compilador (.LEVEL, .MOVS, ...)")));
        };
        match name.as_str() {
            "LEVEL" => {
                let n = match arg {
                    Some(Arg::Num(n)) if (0..1000).contains(&n) => n,
                    _ => return Err(Error::new(format!("{file}: .LEVEL necesita un número de nivel (0-999)"))),
                };
                let (name, bytes) = compile_level(&items, file, n)?;
                out.insert(name, bytes);
            }
            "INVENTARY" | "OBJECT" => {
                out.insert("NIVEL.INV".into(), compile_inventory(&items, file, resources)?);
            }
            "MOVS" => {
                out.insert("NIVEL.MOV".into(), compile_movs(&items));
            }
            "MOUSE" => {
                out.insert("NIVEL.MOU".into(), compile_mouse(&items));
            }
            "ACINVINV" => {
                out.insert("NIVEL.IAC".into(), compile_iac(&items, file)?);
            }
            "TITLE" => {
                out.insert("NIVEL.TIT".into(), compile_title(&items, file)?);
            }
            _ => return Err(Error::new(format!("{file}: orden para el compilador desconocida .{name}"))),
        }
    }
    Ok(out)
}
