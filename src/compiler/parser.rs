//! Parser for EDPACK source files (.ED), the language compiled by ED.EXE.
//!
//! ```text
//! .DIRECTIVE [arg]                  compiler orders (.LEVEL n, .MOVS, ...)
//! NAME(args);                       definition or command
//! ACTION n / ANIMATION n / THING n / OBJINV / DEFMOV(S) / DEFMOUSE ... END;
//! IF(a .op. b) ... [ELSE ...] ENDIF   WHILE(a .op. b) ... ENDWHILE
//! LABEL:   JMP LABEL;  (and JE/JNE/JL/JA/JLE/JAE, and ASLT LABEL; from v2.02)
//! ```
//! Source text is a byte string in DOS code page 850.

use crate::Error;

fn syntax(file: &str, line: usize, msg: impl AsRef<str>) -> Error {
    Error::new(format!("{file}:{line}: {}", msg.as_ref()))
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Str(Vec<u8>),
    Num(i32),
    Id(String),
    Punct(u8),
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    t: Tok,
    line: usize,
}

fn tokenize(src: &[u8], file: &str) -> Result<Vec<Token>, Error> {
    let mut toks = Vec::new();
    let (mut i, mut line, n) = (0, 1, src.len());
    let ident = |c: u8| c.is_ascii_alphabetic() || c == b'_' || c >= 0xc0;
    while i < n {
        let c = src[i];
        match c {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | 0x0c | 0x1a => i += 1,
            b'/' if src.get(i + 1) == Some(&b'/') => {
                while i < n && src[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let mut j = i + 1;
                while j < n && src[j] != b'"' && src[j] != b'\n' {
                    j += 1;
                }
                toks.push(Token { t: Tok::Str(src[i + 1..j].to_vec()), line });
                i = j + 1;
            }
            _ if c.is_ascii_digit() || (c == b'-' && src.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let mut j = i + 1;
                while j < n && src[j].is_ascii_digit() {
                    j += 1;
                }
                let s = std::str::from_utf8(&src[i..j]).unwrap();
                let v = s.parse::<i64>().map_err(|_| syntax(file, line, "Número no válido"))?;
                toks.push(Token { t: Tok::Num(v as i32), line });
                i = j;
            }
            _ if ident(c) => {
                let mut j = i + 1;
                while j < n && (ident(src[j]) || src[j].is_ascii_digit()) {
                    j += 1;
                }
                let s: String = src[i..j].iter().map(|&b| (b as char).to_ascii_uppercase()).collect();
                toks.push(Token { t: Tok::Id(s), line });
                i = j;
            }
            b'(' | b')' | b',' | b';' | b':' | b'[' | b']' | b'.' => {
                toks.push(Token { t: Tok::Punct(c), line });
                i += 1;
            }
            _ => return Err(syntax(file, line, format!("Carácter inesperado '{}'", c as char))),
        }
    }
    toks.push(Token { t: Tok::Eof, line });
    Ok(toks)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Num(i32),
    Str(Vec<u8>),
    Var(i32),
    Id(String),
}

#[derive(Clone, Debug)]
pub struct Cond {
    pub a: Arg,
    pub op: String,
    pub b: Arg,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Nop,
    Label { name: String },
    Jump { op: String, target: String, line: usize },
    If { cond: Cond, then: Vec<Stmt>, els: Vec<Stmt>, line: usize },
    While { cond: Cond, body: Vec<Stmt>, line: usize },
    Cmd { name: String, args: Vec<Arg>, line: usize },
}

#[derive(Clone, Debug)]
pub enum Item {
    Directive {
        name: String,
        arg: Option<Arg>,
        line: usize,
    },
    /// ACTION/ANIMATION/THING n, OBJINV (body) or DEFMOV/DEFMOUSE (values)
    Block {
        kind: String,
        n: i32,
        body: Vec<Stmt>,
        values: Vec<i32>,
        line: usize,
    },
    Stmt(Stmt),
}

const CONDS: [&str; 6] = ["E", "NE", "L", "LE", "A", "AE"];
/// Statements taking a label: the jumps, and ASLT (EDPACK v2.02: skip to the label on a key press).
const JUMPS: [&str; 8] = ["JMP", "JE", "JNE", "JL", "JA", "JLE", "JAE", "ASLT"];

struct Parser<'a> {
    toks: Vec<Token>,
    p: usize,
    file: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.toks[self.p]
    }
    fn next(&mut self) -> Token {
        let t = self.toks[self.p].clone();
        if self.p + 1 < self.toks.len() {
            self.p += 1;
        }
        t
    }
    fn err(&self, msg: impl AsRef<str>) -> Error {
        syntax(self.file, self.peek().line, msg)
    }
    fn accept_punct(&mut self, c: u8) -> bool {
        if self.peek().t == Tok::Punct(c) {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn accept_id(&mut self, s: &str) -> bool {
        if matches!(&self.peek().t, Tok::Id(v) if v == s) {
            self.p += 1;
            true
        } else {
            false
        }
    }
    fn expect_punct(&mut self, c: u8) -> Result<(), Error> {
        if self.accept_punct(c) { Ok(()) } else { Err(self.err(format!("Se esperaba '{}'", c as char))) }
    }
    fn expect_id(&mut self, s: Option<&str>) -> Result<String, Error> {
        match &self.peek().t {
            Tok::Id(v) if s.is_none_or(|s| s == v) => {
                let v = v.clone();
                self.p += 1;
                Ok(v)
            }
            _ => Err(self.err(format!("Se esperaba {}", s.unwrap_or("un identificador")))),
        }
    }
    fn expect_num(&mut self) -> Result<i32, Error> {
        match self.peek().t {
            Tok::Num(v) => {
                self.p += 1;
                Ok(v)
            }
            _ => Err(self.err("Se esperaba un número")),
        }
    }

    fn arg(&mut self) -> Result<Arg, Error> {
        match self.peek().t.clone() {
            Tok::Num(v) => {
                self.p += 1;
                Ok(Arg::Num(v))
            }
            Tok::Str(s) => {
                self.p += 1;
                Ok(Arg::Str(s))
            }
            Tok::Punct(b'[') => {
                self.p += 1;
                let v = self.expect_num()?;
                self.expect_punct(b']')?;
                Ok(Arg::Var(v))
            }
            Tok::Id(s) => {
                self.p += 1;
                Ok(Arg::Id(s))
            }
            _ => Err(self.err("Se esperaba un argumento")),
        }
    }

    fn args(&mut self) -> Result<Vec<Arg>, Error> {
        let mut out = Vec::new();
        if !self.accept_punct(b'(') || self.accept_punct(b')') {
            return Ok(out);
        }
        loop {
            out.push(self.arg()?);
            // tolerate the manual's "a.b" typo as a separator
            if self.accept_punct(b',') || self.accept_punct(b'.') {
                if self.peek().t == Tok::Punct(b')') {
                    break;
                }
                continue;
            }
            break;
        }
        self.expect_punct(b')')?;
        Ok(out)
    }

    fn condition(&mut self) -> Result<Cond, Error> {
        self.expect_punct(b'(')?;
        let a = self.arg()?;
        self.expect_punct(b'.')?;
        let op = self.expect_id(None)?;
        if !CONDS.contains(&op.as_str()) {
            return Err(self.err(format!("Condición desconocida .{op}.")));
        }
        self.expect_punct(b'.')?;
        let b = self.arg()?;
        self.expect_punct(b')')?;
        Ok(Cond { a, op, b })
    }

    fn body(&mut self, close: &[&str]) -> Result<Vec<Stmt>, Error> {
        let mut out = Vec::new();
        loop {
            match &self.peek().t {
                Tok::Eof => return Err(self.err("Fin de archivo inesperado dentro de un bloque")),
                Tok::Id(v) if close.contains(&v.as_str()) => return Ok(out),
                _ => out.push(self.statement()?),
            }
        }
    }

    fn statement(&mut self) -> Result<Stmt, Error> {
        let k = self.next();
        let line = k.line;
        let name = match k.t {
            Tok::Punct(b';') => return Ok(Stmt::Nop),
            Tok::Id(v) => v,
            _ => return Err(syntax(self.file, line, "Se esperaba un comando")),
        };
        if self.accept_punct(b':') {
            return Ok(Stmt::Label { name });
        }
        if name == "IF" {
            let cond = self.condition()?;
            let then = self.body(&["ELSE", "ENDIF"])?;
            let els = if self.accept_id("ELSE") { self.body(&["ENDIF"])? } else { Vec::new() };
            self.expect_id(Some("ENDIF"))?;
            self.accept_punct(b';');
            return Ok(Stmt::If { cond, then, els, line });
        }
        if name == "WHILE" {
            let cond = self.condition()?;
            let body = self.body(&["ENDWHILE"])?;
            self.expect_id(Some("ENDWHILE"))?;
            self.accept_punct(b';');
            return Ok(Stmt::While { cond, body, line });
        }
        if JUMPS.contains(&name.as_str()) && matches!(self.peek().t, Tok::Id(_)) {
            let target = self.expect_id(None)?;
            self.accept_punct(b';');
            return Ok(Stmt::Jump { op: name, target, line });
        }
        let args = self.args()?;
        self.accept_punct(b';');
        Ok(Stmt::Cmd { name, args, line })
    }

    fn number_list(&mut self) -> Result<Vec<i32>, Error> {
        let mut out = Vec::new();
        loop {
            if self.accept_id("END") {
                self.accept_punct(b';');
                return Ok(out);
            }
            match self.next().t {
                Tok::Num(v) => out.push(v),
                Tok::Punct(b',' | b'.' | b';') => {}
                _ => return Err(self.err("Se esperaba un número")),
            }
        }
    }

    fn parse_file(&mut self) -> Result<Vec<Item>, Error> {
        let mut items = Vec::new();
        loop {
            let k = self.peek().clone();
            match k.t {
                Tok::Eof => return Ok(items),
                Tok::Punct(b'.') => {
                    self.p += 1;
                    let name = self.expect_id(None)?;
                    let arg = match self.peek().t.clone() {
                        Tok::Num(v) => {
                            self.p += 1;
                            Some(Arg::Num(v))
                        }
                        Tok::Str(s) => {
                            self.p += 1;
                            Some(Arg::Str(s))
                        }
                        _ => None,
                    };
                    items.push(Item::Directive { name, arg, line: k.line });
                }
                Tok::Punct(b';') => self.p += 1,
                Tok::Id(ref v)
                    if ["ACTION", "ANIMATION", "THING", "OBJINV", "DEFMOV", "DEFMOVS", "DEFMOUSE"]
                        .contains(&v.as_str()) =>
                {
                    self.p += 1;
                    let kind = if v == "DEFMOVS" { "DEFMOV".to_string() } else { v.clone() };
                    let mut block =
                        Item::Block { kind: kind.clone(), n: 0, body: vec![], values: vec![], line: k.line };
                    if let Item::Block { n, body, values, .. } = &mut block {
                        match kind.as_str() {
                            "ACTION" | "ANIMATION" | "THING" => {
                                *n = self.expect_num()?;
                                *body = self.body(&["END"])?;
                                self.expect_id(Some("END"))?;
                                self.accept_punct(b';');
                            }
                            "OBJINV" => {
                                *body = self.body(&["END"])?;
                                self.expect_id(Some("END"))?;
                                self.accept_punct(b';');
                            }
                            _ => *values = self.number_list()?,
                        }
                    }
                    items.push(block);
                }
                Tok::Id(_) => items.push(Item::Stmt(self.statement()?)),
                _ => return Err(self.err("Se esperaba una definición")),
            }
        }
    }
}

pub fn parse_ed(src: &[u8], file: &str) -> Result<Vec<Item>, Error> {
    Parser { toks: tokenize(src, file)?, p: 0, file }.parse_file()
}
