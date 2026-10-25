//! Script code generation for ACTION / ANIMATION bodies, reproducing ED.EXE's
//! lowering (verified word-for-word against the compiled demo game):
//! ```text
//! IF(a .op. b) T [ELSE F] ENDIF -> CMP a,b  J<op> T  JMP F|END  T...  [JMP END  F...]  END:
//! WHILE(a .op. b) B ENDWHILE    -> TOP: CMP a,b  J<op> B  JMP END  B...  JMP TOP  END:
//! ```
//! Jump targets are word offsets relative to the start of the script.

use std::collections::HashMap;

use super::parser::{Arg, Cond, Stmt};
use crate::Error;
use crate::vm::{VAR_REF, op, op_info, opcode_of};

fn cond_jump(c: &str) -> i32 {
    match c {
        "E" => op::JE,
        "NE" => op::JNE,
        "L" => op::JL,
        "A" => op::JA,
        "LE" => op::JLE,
        _ => op::JAE,
    }
}

/// Commands whose arguments are raw variable numbers / constants (no `[]`).
const RAW_ARGS: [i32; 6] = [op::VAR, op::VARVAR, op::ADD, op::ADDVAR, op::SUB, op::SUBVAR];

struct Gen<'a> {
    file: &'a str,
    words: Vec<i32>,
    labels: HashMap<String, usize>,
    fixups: Vec<(usize, String, usize)>,
    anon: usize,
}

impl Gen<'_> {
    fn err(&self, line: usize, msg: impl AsRef<str>) -> Error {
        Error::new(format!("{}:{line}: {}", self.file, msg.as_ref()))
    }
    fn new_label(&mut self) -> String {
        self.anon += 1;
        format!("\0{}", self.anon)
    }
    fn place(&mut self, l: &str) {
        self.labels.insert(l.to_string(), self.words.len());
    }
    fn jump_to(&mut self, opcode: i32, label: &str, line: usize) {
        self.words.push(opcode);
        self.fixups.push((self.words.len(), label.to_string(), line));
        self.words.push(0);
    }
    fn operand(&self, a: &Arg, line: usize) -> Result<i32, Error> {
        match a {
            Arg::Num(n) => Ok(*n),
            Arg::Var(v) => Ok(VAR_REF + v),
            _ => Err(self.err(line, "Argumento no válido")),
        }
    }
    fn cmp(&mut self, c: &Cond, line: usize) -> Result<(), Error> {
        let (a, b) = (self.operand(&c.a, line)?, self.operand(&c.b, line)?);
        self.words.extend([op::CMP, a, b]);
        Ok(())
    }

    fn emit(&mut self, stmts: &[Stmt]) -> Result<(), Error> {
        for s in stmts {
            match s {
                Stmt::Nop => {}
                Stmt::Label { name } => self.place(name),
                Stmt::Jump { op: o, target, line } => {
                    let code = opcode_of(o).ok_or_else(|| self.err(*line, "Salto desconocido"))?;
                    self.jump_to(code, target, *line);
                }
                Stmt::If { cond, then, els, line } => {
                    let (l_then, l_else, l_end) = (self.new_label(), self.new_label(), self.new_label());
                    self.cmp(cond, *line)?;
                    self.jump_to(cond_jump(&cond.op), &l_then, *line);
                    self.jump_to(op::JMP, if els.is_empty() { &l_end } else { &l_else }, *line);
                    self.place(&l_then);
                    self.emit(then)?;
                    if !els.is_empty() {
                        self.jump_to(op::JMP, &l_end, *line);
                        self.place(&l_else);
                        self.emit(els)?;
                    }
                    self.place(&l_end);
                }
                Stmt::While { cond, body, line } => {
                    let (l_top, l_body, l_end) = (self.new_label(), self.new_label(), self.new_label());
                    self.place(&l_top);
                    self.cmp(cond, *line)?;
                    self.jump_to(cond_jump(&cond.op), &l_body, *line);
                    self.jump_to(op::JMP, &l_end, *line);
                    self.place(&l_body);
                    self.emit(body)?;
                    self.jump_to(op::JMP, &l_top, *line);
                    self.place(&l_end);
                }
                Stmt::Cmd { name, args, line } => {
                    let line = *line;
                    if name == "CMP" || name == "CMPVAR" {
                        // CMP(var, value) / CMPVAR(var, var)
                        let num = |a: Option<&Arg>| match a {
                            Some(Arg::Num(n)) | Some(Arg::Var(n)) => Ok(*n),
                            _ => Err(self.err(line, "Argumento no válido")),
                        };
                        let x = num(args.first())?;
                        let y = if name == "CMPVAR" {
                            VAR_REF + num(args.get(1))?
                        } else {
                            self.operand(args.get(1).ok_or_else(|| self.err(line, "Faltan argumentos"))?, line)?
                        };
                        self.words.extend([op::CMP, VAR_REF + x, y]);
                        continue;
                    }
                    let code = opcode_of(name).ok_or_else(|| self.err(line, format!("Comando desconocido {name}")))?;
                    let (_, argc) = op_info(code).unwrap();
                    if args.len() != argc {
                        return Err(self.err(line, format!("{name} necesita {argc} argumentos")));
                    }
                    self.words.push(code);
                    for a in args {
                        if RAW_ARGS.contains(&code) && matches!(a, Arg::Var(_)) {
                            return Err(self.err(line, format!("{name} no admite [variables]")));
                        }
                        let v = self.operand(a, line)?;
                        self.words.push(v);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Compile a script body to words, terminated by END (0).
pub fn compile_script(body: &[Stmt], file: &str) -> Result<Vec<i32>, Error> {
    let mut g = Gen { file, words: Vec::new(), labels: HashMap::new(), fixups: Vec::new(), anon: 0 };
    g.emit(body)?;
    g.words.push(op::END);
    for (at, label, line) in std::mem::take(&mut g.fixups) {
        let target = *g.labels.get(&label).ok_or_else(|| g.err(line, format!("Etiqueta no definida {label}")))?;
        g.words[at] = target as i32;
    }
    if g.words.len() > 256 {
        return Err(Error::new(format!(
            "{file}: script de más de 256 palabras (el intérprete usa un contador de 8 bits)"
        )));
    }
    Ok(g.words)
}
