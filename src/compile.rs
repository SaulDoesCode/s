use crate::rt::*;
use std::collections::HashMap;
use std::fmt::Write as _;

pub const RT_SRC: &str = include_str!("rt.rs");

type St = Option<Vec<Option<String>>>;

struct Gen {
    it: Interner,
    srcs: Vec<String>,
    idx: HashMap<String, usize>,
    done: usize,
    fns: String,
}

fn f64_lit(x: f64) -> String {
    if x.is_nan() {
        "f64::NAN".into()
    } else if x == f64::INFINITY {
        "f64::INFINITY".into()
    } else if x == f64::NEG_INFINITY {
        "f64::NEG_INFINITY".into()
    } else {
        format!("{:?}", x)
    }
}

fn val_lit(v: &Val) -> String {
    match v {
        Val::N(x) => format!("Val::N({})", f64_lit(*x)),
        Val::L(s) => format!("Val::L({:?})", s),
        Val::R(s) | Val::K(s, _) => format!("Val::L({:?})", &**s),
        Val::W(s, _) => format!("Val::L({:?})", s),
    }
}

fn push_st(st: &mut St, v: Option<String>) {
    if let Some(s) = st {
        s.push(v);
    }
}

impl Gen {
    fn chunk(&mut self, src: &str) -> usize {
        if let Some(&i) = self.idx.get(src) {
            return i;
        }
        let i = self.srcs.len();
        self.srcs.push(src.to_string());
        self.idx.insert(src.to_string(), i);
        i
    }

    fn drain(&mut self) -> Result<(), String> {
        while self.done < self.srcs.len() {
            let i = self.done;
            self.done += 1;
            let src = self.srcs[i].clone();
            let ops = parse(&src, &mut self.it).map_err(|e| format!("chunk {}: {}", i, e))?;
            let mut code = String::new();
            let mut st: St = Some(Vec::new());
            let n = ops.len();
            let mut k = 0;
            while k < n {
                if let Some(m) = self.fuse(&ops[k..], &mut code) {
                    k += m;
                    st = Some(Vec::new());
                    continue;
                }
                self.emit(&ops[k], k + 1 == n, &mut st, &mut code, 1);
                k += 1;
            }
            let _ = write!(self.fns, "fn c{}(rt: &mut Rt) -> Flow {{\n{}    Flow::Done\n}}\n\n", i, code);
        }
        Ok(())
    }

    /// Guarded superinstruction for `operands... command` on an empty stack.
    /// Fast path computes directly; the slow path is the plain op sequence.
    fn fuse(&mut self, ops: &[Op], o: &mut String) -> Option<usize> {
        let m = ops.iter().take(4).take_while(|op| matches!(op, Op::Push(_) | Op::Word(..) | Op::Lookup(_))).count();
        let c = ops.get(m)?;
        let numeric = |c: &Op| match c {
            Op::Cmd(Cmd::Add | Cmd::Sub | Cmd::Mul | Cmd::Div | Cmd::Pow | Cmd::Mod) => true,
            Op::Cmd(Cmd::NumEq | Cmd::Lt | Cmd::Gt | Cmd::Le | Cmd::Ge) => true,
            Op::SetNum(_) | Op::Inc(_) | Op::Dec(_) | Op::Cmd(Cmd::SetN) => true,
            _ => false,
        };
        let textual = matches!(c, Op::Cmd(Cmd::Eq | Cmd::Ne | Cmd::AndEq | Cmd::AndNe));
        if !numeric(c) && !textual {
            return None;
        }
        let static_sym = |op: &Op, it: &mut Interner| -> Option<Sym> {
            match op {
                Op::Word(_, k, _) => Some(*k),
                Op::Push(v) => Some(it.sym(&v.text())),
                _ => None,
            }
        };
        let arity = match c {
            Op::SetNum(_) | Op::Inc(_) | Op::Dec(_) => (1, 1),
            Op::Cmd(Cmd::SetN) => (2, 2),
            Op::Cmd(Cmd::Add | Cmd::Sub | Cmd::Mul | Cmd::Div | Cmd::Pow | Cmd::Mod) => (2, 3),
            _ => (2, 2),
        };
        if m < arity.0 || m > arity.1 {
            return None;
        }
        let dest = match c {
            Op::SetNum(k) | Op::Inc(k) | Op::Dec(k) => *k,
            Op::Cmd(Cmd::SetN) => static_sym(&ops[1], &mut self.it)?,
            Op::Cmd(_) if m == 3 => static_sym(&ops[2], &mut self.it)?,
            _ => S_EQ,
        };
        let nvals = match c {
            Op::Cmd(Cmd::SetN) => 1,
            _ if m == 3 => 2,
            _ => m,
        };
        let mut f = String::new();
        let _ = writeln!(f, "    '{{");
        let _ = writeln!(f, "        'slow: {{");
        let _ = writeln!(f, "            if !(rt.args.is_empty() && rt.repl.is_empty()) {{ break 'slow; }}");
        for (i, op) in ops[..nvals].iter().enumerate() {
            let e = if textual {
                match op {
                    Op::Push(v) => val_lit(v),
                    Op::Word(t, k, None) => format!("Val::W({:?}, Sym({}))", &**t, k.0),
                    Op::Word(_, _, Some(x)) => format!("Val::N({})", f64_lit(*x)),
                    Op::Lookup(k) => format!("match rt.val_of(Sym({})) {{ Some(v) => v, None => break 'slow }}", k.0),
                    _ => unreachable!(),
                }
            } else {
                match op {
                    Op::Push(v) => f64_lit(v.num()),
                    Op::Word(t, _, None) => f64_lit(parse_num(t)),
                    Op::Word(_, _, Some(x)) => f64_lit(*x),
                    Op::Lookup(k) => format!("match rt.num_of(Sym({})) {{ Some(x) => x, None => break 'slow }}", k.0),
                    _ => unreachable!(),
                }
            };
            let ty = if textual { "Val" } else { "f64" };
            let _ = writeln!(f, "            let a{}: {} = {};", i, ty, e);
        }
        let d = dest.0;
        let body = match c {
            Op::Cmd(Cmd::Add) => format!("rt.nset(Sym({}), a0 + a1);", d),
            Op::Cmd(Cmd::Sub) => format!("rt.nset(Sym({}), a0 - a1);", d),
            Op::Cmd(Cmd::Mul) => format!("rt.nset(Sym({}), a0 * a1);", d),
            Op::Cmd(Cmd::Div) => format!("rt.nset(Sym({}), a0 / a1);", d),
            Op::Cmd(Cmd::Pow) => format!("rt.nset(Sym({}), a0.powf(a1));", d),
            Op::Cmd(Cmd::Mod) => format!("rt.nset(Sym({}), a0.rem_euclid(a1));", d),
            Op::Cmd(Cmd::NumEq) => "rt.ow = a0 == a1;".into(),
            Op::Cmd(Cmd::Lt) => "rt.ow = a0 < a1;".into(),
            Op::Cmd(Cmd::Gt) => "rt.ow = a0 > a1;".into(),
            Op::Cmd(Cmd::Le) => "rt.ow = a0 <= a1;".into(),
            Op::Cmd(Cmd::Ge) => "rt.ow = a0 >= a1;".into(),
            Op::Cmd(Cmd::Eq) => "rt.ow = val_eq(&a0, &a1);".into(),
            Op::Cmd(Cmd::Ne) => "rt.ow = !val_eq(&a0, &a1);".into(),
            Op::Cmd(Cmd::AndEq) => "rt.ow = rt.ow && val_eq(&a0, &a1);".into(),
            Op::Cmd(Cmd::AndNe) => "rt.ow = rt.ow && !val_eq(&a0, &a1);".into(),
            Op::Cmd(Cmd::SetN) | Op::SetNum(_) => format!("rt.nset(Sym({}), a0);", d),
            Op::Inc(_) => format!("rt.add_num(Sym({}), a0);", d),
            Op::Dec(_) => format!("rt.add_num(Sym({}), -a0);", d),
            _ => unreachable!(),
        };
        let _ = writeln!(f, "            {}", body);
        let _ = writeln!(f, "            break 'fused;");
        let _ = writeln!(f, "        }}");
        let mut st: St = Some(Vec::new());
        let mut slow = String::new();
        for op in &ops[..=m] {
            self.emit(op, false, &mut st, &mut slow, 2);
        }
        f.push_str(&slow);
        let _ = writeln!(f, "    }}");
        o.push_str(&f.replacen("'{", "'fused: {", 1));
        Some(m + 1)
    }

    fn hint(&mut self, st: &St) -> Option<usize> {
        let v = st.as_ref()?;
        let mut parts = Vec::with_capacity(v.len());
        for x in v {
            parts.push(x.as_deref()?);
        }
        let body = parts.join(" ").replace('|', "~");
        if parse(&body, &mut self.it).is_err() {
            return None;
        }
        Some(self.chunk(&body))
    }

    fn emit(&mut self, op: &Op, tail: bool, st: &mut St, o: &mut String, d: usize) {
        let p = "    ".repeat(d);
        match op {
            Op::Push(v) => {
                let _ = writeln!(o, "{}rt.push({});", p, val_lit(v));
                push_st(st, Some(v.text().into_owned()));
            }
            Op::Interp(parts) => {
                let _ = writeln!(o, "{}{{", p);
                let _ = writeln!(o, "{}    let mut s = String::new();", p);
                for part in parts {
                    match part {
                        Part::T(t) => {
                            let _ = writeln!(o, "{}    s.push_str({:?});", p, t);
                        }
                        Part::V(k) => {
                            let _ = writeln!(o, "{}    rt.interp(&mut s, Sym({}));", p, k.0);
                        }
                    }
                }
                let _ = writeln!(o, "{}    rt.push_string(s);", p);
                let _ = writeln!(o, "{}}}", p);
                push_st(st, None);
            }
            Op::Word(t, k, n) => {
                match n {
                    Some(x) => {
                        let _ = writeln!(o, "{}rt.word_n({}, {:?});", p, f64_lit(*x), &**t);
                    }
                    None => {
                        let _ = writeln!(o, "{}rt.word({:?}, Sym({}));", p, &**t, k.0);
                    }
                }
                push_st(st, Some(t.to_string()));
            }
            Op::Cmd(Cmd::Ret) => {
                let _ = writeln!(o, "{}return Flow::Done;", p);
                *st = None;
            }
            Op::Cmd(c) => {
                let _ = writeln!(o, "{}rt.cmd(Cmd::{:?});", p, c);
                *st = if c.runs_code() { None } else { Some(Vec::new()) };
            }
            Op::Lookup(k) => {
                let _ = writeln!(o, "{}if rt.lookup(Sym({})) {{ return Flow::Done; }}", p, k.0);
                *st = None;
            }
            Op::Def(k) => {
                let h = match self.hint(st) {
                    Some(i) => format!("Some({})", i),
                    None => "None".into(),
                };
                let _ = writeln!(o, "{}rt.def_script(Sym({}), {});", p, k.0, h);
                *st = Some(Vec::new());
            }
            Op::SetMem(k) | Op::SetNum(k) | Op::Inc(k) | Op::Dec(k) => {
                let m = match op {
                    Op::SetMem(_) => "set_mem",
                    Op::SetNum(_) => "set_num",
                    Op::Inc(_) => "inc",
                    _ => "dec",
                };
                let _ = writeln!(o, "{}rt.{}(Sym({}));", p, m, k.0);
                *st = Some(Vec::new());
            }
            Op::Call(k) => {
                if tail {
                    let _ = writeln!(o, "{}return rt.tail_site(Sym({}));", p, k.0);
                } else {
                    let _ = writeln!(o, "{}rt.call_site(Sym({}));", p, k.0);
                }
                *st = None;
            }
            Op::CallPass(k) => {
                if tail {
                    let _ = writeln!(o, "{}return Flow::Tail(Sym({}));", p, k.0);
                } else {
                    let _ = writeln!(o, "{}rt.call(Sym({}));", p, k.0);
                }
                *st = None;
            }
            Op::Cond(t, inner) => {
                let _ = writeln!(o, "{}if {}rt.ow {{", p, if *t { "" } else { "!" });
                let mut ist = st.clone();
                self.emit(inner, tail, &mut ist, o, d + 1);
                let _ = writeln!(o, "{}}}", p);
                *st = None;
            }
        }
    }
}

/// main: path and text of the entry file. embeds: (path, text) pairs compiled in and
/// served by `t`, `***` and `name.` in place of reading the path at run time.
pub fn compile(main: (&str, &str), embeds: &[(String, String)]) -> Result<String, String> {
    let mut g = Gen { it: Interner::new(), srcs: Vec::new(), idx: HashMap::new(), done: 0, fns: String::new() };
    parse(main.1, &mut g.it).map_err(|e| format!("{}: {}", main.0, e))?;
    g.chunk(main.1);
    let mut files = Vec::new();
    for (path, text) in embeds {
        parse(text, &mut g.it).map_err(|e| format!("{}: {}", path, e))?;
        files.push((path.clone(), g.chunk(text)));
    }
    g.drain()?;

    let mut out = String::new();
    let _ = writeln!(out, "// generated by s from {}", main.0);
    out.push_str(RT_SRC);
    out.push_str("\n// program\n\n");
    out.push_str(&g.fns);
    out.push_str("static SYMS: &[&str] = &[\n");
    for n in &g.it.names {
        let _ = writeln!(out, "    {:?},", &**n);
    }
    out.push_str("];\n\nstatic CHUNKS: &[(&str, Chunk)] = &[\n");
    for (i, s) in g.srcs.iter().enumerate() {
        let _ = writeln!(out, "    ({:?}, c{}),", s, i);
    }
    out.push_str("];\n\nstatic FILES: &[(&str, usize)] = &[\n");
    for (p, i) in &files {
        let _ = writeln!(out, "    ({:?}, {}),", p, i);
    }
    out.push_str("];\n\nstatic PROGRAM: Program = Program::new(SYMS, CHUNKS, FILES);\n\n");
    out.push_str("fn main() {\n    s_main(&PROGRAM, Entry::Chunk(0), std::env::args().skip(1).collect())\n}\n");
    Ok(out)
}
