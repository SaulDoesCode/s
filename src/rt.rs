// s runtime. Embedded verbatim at the top of every compiled program.
#![allow(dead_code, unused_imports, unused_variables, unused_mut, unreachable_code, unused_macros, clippy::all)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufWriter, Read, Seek, SeekFrom, Write};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;

pub const MAIN_STACK: usize = 256 << 20;
pub const WORKER_STACK: usize = 64 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Sym(pub u32);

pub const RESERVED: [&str; 14] = [
    "=", "n", "r", "rc", "rn", "$", "err", "eof", "out", "threads", "_", "args", "argc", "?",
];
pub const S_EQ: Sym = Sym(0);
pub const S_N: Sym = Sym(1);
pub const S_R: Sym = Sym(2);
pub const S_RC: Sym = Sym(3);
pub const S_RN: Sym = Sym(4);
pub const S_DOLLAR: Sym = Sym(5);
pub const S_ERR: Sym = Sym(6);
pub const S_EOF: Sym = Sym(7);
pub const S_OUT: Sym = Sym(8);
pub const S_THREADS: Sym = Sym(9);
pub const S_UNDER: Sym = Sym(10);
pub const S_ARGS: Sym = Sym(11);
pub const S_ARGC: Sym = Sym(12);
pub const S_Q: Sym = Sym(13);

#[derive(Clone, Debug)]
pub enum Val {
    N(f64),
    L(&'static str),
    R(Rc<str>),
    W(&'static str, Sym),
    K(Rc<str>, Sym),
}

impl Val {
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            Val::N(x) => Cow::Owned(fmt_num(*x)),
            Val::L(s) => Cow::Borrowed(s),
            Val::R(s) | Val::K(s, _) => Cow::Borrowed(&**s),
            Val::W(s, _) => Cow::Borrowed(s),
        }
    }
    pub fn num(&self) -> f64 {
        match self {
            Val::N(x) => *x,
            Val::L(s) | Val::W(s, _) => parse_num(s),
            Val::R(s) | Val::K(s, _) => parse_num(s),
        }
    }
    pub fn of(s: String) -> Val {
        match canon_num(&s) {
            Some(x) => Val::N(x),
            None => Val::R(Rc::from(s)),
        }
    }
}

pub fn fmt_num(x: f64) -> String {
    if x == 0.0 { "0".to_string() } else { x.to_string() }
}

pub fn parse_num(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

pub fn canon_num(s: &str) -> Option<f64> {
    let x = s.parse::<f64>().ok()?;
    if fmt_num(x) == s { Some(x) } else { None }
}

pub fn val_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::N(x), Val::N(y)) if !x.is_nan() && !y.is_nan() => x == y,
        _ => a.text() == b.text(),
    }
}

#[derive(Default, Clone, Copy)]
pub struct Fx(u64);

impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, b: &[u8]) {
        for c in b.chunks(8) {
            let mut v = 0u64;
            for (i, &x) in c.iter().enumerate() {
                v |= (x as u64) << (8 * i);
            }
            self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
}

pub type FxMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<Fx>>;

pub struct Interner {
    pub map: FxMap<Rc<str>, u32>,
    pub names: Vec<Rc<str>>,
}

impl Interner {
    pub fn new() -> Interner {
        let mut it = Interner { map: FxMap::default(), names: Vec::new() };
        for n in RESERVED.iter() {
            it.sym(n);
        }
        it
    }
    pub fn sym(&mut self, s: &str) -> Sym {
        if let Some(&k) = self.map.get(s) {
            return Sym(k);
        }
        let rc: Rc<str> = Rc::from(s);
        let k = self.names.len() as u32;
        self.names.push(rc.clone());
        self.map.insert(rc, k);
        Sym(k)
    }
    pub fn name(&self, s: Sym) -> &str {
        &self.names[s.0 as usize]
    }
}

macro_rules! cmds {
    ($($w:literal => $c:ident,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Cmd { $($c,)* }
        pub fn cmd_of(w: &str) -> Option<Cmd> {
            match w { $($w => Some(Cmd::$c),)* _ => None }
        }
        impl Cmd {
            pub fn word(self) -> &'static str { match self { $(Cmd::$c => $w,)* } }
        }
        pub const CMD_WORDS: &[&str] = &[$($w,)*];
    };
}

cmds! {
    ">" => Print, ">_" => PrintRaw, "<" => Shell, "<_" => ReadIn,
    "w" => Write, "+w" => Append, "mv" => Move, "r" => Read, "r@" => ReadAt,
    "?)" => Match, "&&&" => Repeat, "***" => Par,
    "n" => SetN, "+" => Add, "-" => Sub, "*" => Mul, "/" => Div, "^" => Pow, "%" => Mod,
    "!!" => Not, "=" => Eq, "!=" => Ne, ";" => AndNe, "&" => AndEq,
    "==" => NumEq, "<<" => Lt, ">>" => Gt, "<<=" => Le, ">>=" => Ge,
    "l" => LPush, "-l" => LPop, "[l]" => LGet, "l=" => LSet, "l-" => LDel,
    "#l" => LLen, "/l" => Split, "l/" => Join, "#s" => SLen,
    "f" => Open, "+f" => FWriteAt, "@f" => FReadAt, "f-" => FClose,
    "f_" => FWriteLn, "_f" => FReadLn, "?eof" => Eof,
    "t" => Spawn, "-t" => Kill, "<t" => Send, ">t" => Recv,
    "#prng" => PrngN, "`prng" => PrngS,
    "><" => Replace, "//" => Comment, "<-" => Ret, "<--" => Exit,
}

impl Cmd {
    pub fn runs_code(self) -> bool {
        matches!(self, Cmd::Match | Cmd::Par | Cmd::Recv | Cmd::Open | Cmd::FWriteAt | Cmd::FWriteLn)
    }
}

#[derive(Debug)]
pub enum Part {
    T(String),
    V(Sym),
}

#[derive(Debug)]
pub enum Op {
    Push(Val),
    Interp(Vec<Part>),
    Word(Rc<str>, Sym, Option<f64>),
    Cmd(Cmd),
    Lookup(Sym),
    Def(Sym),
    SetMem(Sym),
    SetNum(Sym),
    Inc(Sym),
    Dec(Sym),
    Call(Sym),
    CallPass(Sym),
    Cond(bool, Box<Op>),
}

pub fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r')
}

pub fn classify(w: &str, it: &mut Interner) -> Op {
    if let Some(c) = cmd_of(w) {
        return Op::Cmd(c);
    }
    let last = w.chars().last().unwrap();
    let stem = &w[..w.len() - last.len_utf8()];
    if !stem.is_empty() {
        match last {
            '?' => return Op::Cond(true, Box::new(classify(stem, it))),
            '!' => return Op::Cond(false, Box::new(classify(stem, it))),
            '~' => return Op::Lookup(it.sym(stem)),
            '$' => return Op::Def(it.sym(stem)),
            '`' => return Op::SetMem(it.sym(stem)),
            '#' => return Op::SetNum(it.sym(stem)),
            ';' => return Op::Inc(it.sym(stem)),
            ':' => return Op::Dec(it.sym(stem)),
            '.' => {
                if stem.len() > 1 && stem.ends_with('.') {
                    return Op::CallPass(it.sym(&stem[..stem.len() - 1]));
                }
                return Op::Call(it.sym(stem));
            }
            _ => {}
        }
    }
    Op::Word(Rc::from(w), it.sym(w), canon_num(w))
}

fn brace(cs: &[char], i: usize, it: &mut Interner) -> Result<(Vec<Part>, usize), String> {
    let n = cs.len();
    let mut depth = 1;
    let mut j = i + 1;
    let mut parts = Vec::new();
    let mut cur = String::new();
    while j < n {
        let c = cs[j];
        match c {
            '{' => {
                depth += 1;
                cur.push(c);
                j += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if !cur.is_empty() {
                        parts.push(Part::T(cur));
                    }
                    return Ok((parts, j + 1));
                }
                cur.push(c);
                j += 1;
            }
            '~' if depth == 1 => {
                let mut k = j + 1;
                while k < n && cs[k] != '~' && cs[k] != '{' && cs[k] != '}' && !is_ws(cs[k]) {
                    k += 1;
                }
                if k < n && cs[k] == '~' {
                    if k == j + 1 {
                        cur.push('~');
                    } else {
                        if !cur.is_empty() {
                            parts.push(Part::T(std::mem::take(&mut cur)));
                        }
                        let name: String = cs[j + 1..k].iter().collect();
                        parts.push(Part::V(it.sym(&name)));
                    }
                    j = k + 1;
                } else {
                    cur.push('~');
                    j += 1;
                }
            }
            _ => {
                cur.push(c);
                j += 1;
            }
        }
    }
    Err("unclosed {".to_string())
}

fn lit_op(prefix: String, parts: Vec<Part>) -> Op {
    let mut ps: Vec<Part> = Vec::new();
    for p in std::iter::once(Part::T(prefix)).chain(parts) {
        match (ps.last_mut(), p) {
            (_, Part::T(t)) if t.is_empty() => {}
            (Some(Part::T(a)), Part::T(t)) => a.push_str(&t),
            (_, p) => ps.push(p),
        }
    }
    if ps.iter().all(|p| matches!(p, Part::T(_))) {
        let t = match ps.pop() {
            Some(Part::T(t)) => t,
            _ => String::new(),
        };
        Op::Push(Val::of(t))
    } else {
        Op::Interp(ps)
    }
}

pub fn parse(src: &str, it: &mut Interner) -> Result<Vec<Op>, String> {
    let cs: Vec<char> = src.chars().collect();
    let n = cs.len();
    let mut ops = Vec::new();
    let mut i = 0;
    while i < n {
        if is_ws(cs[i]) {
            i += 1;
            continue;
        }
        let mut word = String::new();
        let mut done = false;
        while i < n && !is_ws(cs[i]) {
            match cs[i] {
                '{' => {
                    let (parts, j) = brace(&cs, i, it)?;
                    ops.push(lit_op(std::mem::take(&mut word), parts));
                    i = j;
                    done = true;
                    break;
                }
                '\'' => {
                    let mut j = i + 1;
                    while j < n && cs[j] != '\'' {
                        j += 1;
                    }
                    if j >= n {
                        return Err("unclosed '".to_string());
                    }
                    let t: String = cs[i + 1..j].iter().collect();
                    let mut w = std::mem::take(&mut word);
                    w.push_str(&t);
                    ops.push(Op::Push(Val::of(w)));
                    i = j + 1;
                    done = true;
                    break;
                }
                c => {
                    word.push(c);
                    i += 1;
                }
            }
        }
        if !done && !word.is_empty() {
            ops.push(classify(&word, it));
        }
    }
    Ok(ops)
}

pub type Chunk = fn(&mut Rt) -> Flow;

pub enum Flow {
    Done,
    Tail(Sym),
}

#[derive(Clone)]
pub enum Script {
    C(Chunk),
    U(Rc<Vec<Op>>),
}

pub struct Program {
    pub syms: &'static [&'static str],
    pub chunks: &'static [(&'static str, Chunk)],
    pub files: &'static [(&'static str, usize)],
    pub cache: OnceLock<FxMap<&'static str, Chunk>>,
}

impl Program {
    pub const fn new(
        syms: &'static [&'static str],
        chunks: &'static [(&'static str, Chunk)],
        files: &'static [(&'static str, usize)],
    ) -> Program {
        Program { syms, chunks, files, cache: OnceLock::new() }
    }
    pub fn find(&self, src: &str) -> Option<Chunk> {
        if self.chunks.is_empty() {
            return None;
        }
        self.cache
            .get_or_init(|| self.chunks.iter().map(|&(s, f)| (s, f)).collect())
            .get(src)
            .copied()
    }
    pub fn file(&self, path: &str) -> Option<Chunk> {
        self.files.iter().find(|f| f.0 == path).map(|f| self.chunks[f.1].1)
    }
}

pub static EMPTY: Program = Program::new(&[], &[], &[]);

pub struct FileH {
    pub f: File,
    pub eof: bool,
}

pub struct Worker {
    pub tx: Sender<String>,
    pub rx: Receiver<String>,
}

pub struct Rng(pub [u64; 4]);

fn splitmix(z: &mut u64) -> u64 {
    *z = z.wrapping_add(0x9E3779B97F4A7C15);
    let mut x = *z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
    x ^ (x >> 31)
}

impl Rng {
    pub fn seeded(a: u64, b: u64) -> Rng {
        let mut z = a ^ b.rotate_left(32);
        let mut s = [0u64; 4];
        for x in s.iter_mut() {
            *x = splitmix(&mut z);
        }
        Rng(s)
    }
    pub fn next(&mut self) -> u64 {
        let s = &mut self.0;
        let r = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        r
    }
    pub fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn os_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(C.fetch_add(1, Ordering::Relaxed));
    h.finish()
}

fn grow<T>(v: &mut Vec<Option<T>>, i: usize) -> &mut Option<T> {
    if v.len() <= i {
        v.resize_with(i + 1, || None);
    }
    &mut v[i]
}

fn open_mode(path: &str, mode: &str) -> io::Result<File> {
    let m: String = mode.chars().filter(|&c| c != 'b').collect();
    let mut o = OpenOptions::new();
    match m.as_str() {
        "r" => o.read(true),
        "w" => o.write(true).create(true).truncate(true),
        "a" => o.append(true).create(true),
        "r+" => o.read(true).write(true),
        "w+" => o.read(true).write(true).create(true).truncate(true),
        "a+" => o.read(true).append(true).create(true),
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("bad mode '{}'", mode))),
    };
    o.open(path)
}

fn read_line_h(h: &mut FileH) -> io::Result<String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    let mut got = false;
    loop {
        let n = h.f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        got = true;
        if let Some(p) = buf[..n].iter().position(|&b| b == b'\n') {
            out.extend_from_slice(&buf[..p]);
            let back = (n - p - 1) as i64;
            if back > 0 {
                h.f.seek(SeekFrom::Current(-back))?;
            }
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    h.eof = !got;
    if out.last() == Some(&b'\r') {
        out.pop();
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn shell(c: &str) -> io::Result<std::process::Output> {
    let mut cmd = if cfg!(windows) {
        let mut k = Command::new("cmd");
        k.arg("/C").arg(c);
        k
    } else {
        let mut k = Command::new("sh");
        k.arg("-c").arg(c);
        k
    };
    cmd.stdin(Stdio::inherit()).stderr(Stdio::inherit()).output()
}

pub struct Rt {
    pub prog: &'static Program,
    pub it: Interner,
    pub args: Vec<Val>,
    pub ow: bool,
    pub mem: Vec<Option<Val>>,
    pub nums: Vec<Option<f64>>,
    pub scripts: Vec<Option<Script>>,
    pub lists: Vec<Option<Vec<Val>>>,
    pub repl: FxMap<Rc<str>, Val>,
    pub files: HashMap<u32, FileH>,
    pub workers: Vec<Option<Worker>>,
    pub units: FxMap<String, Rc<Vec<Op>>>,
    pub rng: Rng,
    pub out: BufWriter<io::Stdout>,
}

impl Rt {
    pub fn new(prog: &'static Program) -> Rt {
        let mut it = Interner::new();
        for s in prog.syms {
            it.sym(s);
        }
        let n = it.names.len();
        Rt {
            prog,
            it,
            args: Vec::with_capacity(16),
            ow: false,
            mem: vec![None; n],
            nums: vec![None; n],
            scripts: vec![None; n],
            lists: Vec::new(),
            repl: FxMap::default(),
            files: HashMap::new(),
            workers: Vec::new(),
            units: FxMap::default(),
            rng: Rng::seeded(os_u64(), os_u64()),
            out: BufWriter::with_capacity(1 << 16, io::stdout()),
        }
    }

    pub fn set_argv(&mut self, argv: Vec<String>) {
        let n = argv.len() as f64;
        let l: Vec<Val> = argv.into_iter().map(Val::of).collect();
        *grow(&mut self.lists, S_ARGS.0 as usize) = Some(l);
        self.nset(S_ARGC, n);
    }

    pub fn fail(&mut self, msg: String) -> ! {
        let _ = self.out.flush();
        eprintln!("\ns: {}", msg);
        std::process::exit(1)
    }

    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }

    // stack

    #[inline]
    pub fn push(&mut self, v: Val) {
        self.args.push(v);
    }
    pub fn push_string(&mut self, s: String) {
        self.args.push(Val::of(s));
    }
    #[inline]
    pub fn word(&mut self, t: &'static str, k: Sym) {
        if !self.repl.is_empty() {
            if let Some(r) = self.repl.get(t) {
                let r = r.clone();
                self.args.push(r);
                return;
            }
        }
        self.args.push(Val::W(t, k));
    }
    #[inline]
    pub fn word_n(&mut self, x: f64, t: &'static str) {
        if !self.repl.is_empty() {
            if let Some(r) = self.repl.get(t) {
                let r = r.clone();
                self.args.push(r);
                return;
            }
        }
        self.args.push(Val::N(x));
    }
    fn word_rc(&mut self, t: &Rc<str>, k: Sym, n: Option<f64>) {
        if !self.repl.is_empty() {
            if let Some(r) = self.repl.get(&**t) {
                let r = r.clone();
                self.args.push(r);
                return;
            }
        }
        self.args.push(match n {
            Some(x) => Val::N(x),
            None => Val::K(t.clone(), k),
        });
    }

    fn need(&mut self, n: usize, c: Cmd) {
        if self.args.len() < n {
            let m = format!("{}: needs {} args, stack has {}", c.word(), n, self.args.len());
            self.fail(m)
        }
    }
    fn t(&self, i: usize) -> String {
        self.args[i].text().into_owned()
    }
    fn opt_t(&self, i: usize) -> Option<String> {
        self.args.get(i).map(|v| v.text().into_owned())
    }
    fn x(&self, i: usize) -> f64 {
        self.args[i].num()
    }
    fn arg_sym(&mut self, i: usize) -> Sym {
        match &self.args[i] {
            Val::W(_, k) | Val::K(_, k) => *k,
            v => {
                let t = v.text();
                self.it.sym(&t)
            }
        }
    }
    fn dest(&mut self, i: usize, def: Sym) -> Sym {
        if self.args.len() > i { self.arg_sym(i) } else { def }
    }

    // variables

    #[inline]
    pub fn nset(&mut self, s: Sym, x: f64) {
        *grow(&mut self.nums, s.0 as usize) = Some(x);
    }
    pub fn mset(&mut self, s: Sym, v: Val) {
        *grow(&mut self.mem, s.0 as usize) = Some(v);
    }
    pub fn take_mem(&mut self, s: Sym) -> Option<String> {
        self.mem.get_mut(s.0 as usize).and_then(|v| v.take()).map(|v| v.text().into_owned())
    }
    pub fn get_val(&mut self, s: Sym) -> Val {
        let i = s.0 as usize;
        if let Some(Some(v)) = self.mem.get(i) {
            return v.clone();
        }
        if let Some(Some(x)) = self.nums.get(i) {
            return Val::N(*x);
        }
        if s == S_Q {
            return Val::R(Rc::from(os_u64().to_string()));
        }
        Val::L("404")
    }
    pub fn plain(v: &Val) -> bool {
        match v {
            Val::L(t) => cmd_of(t).is_none(),
            Val::R(t) => cmd_of(t).is_none(),
            _ => true,
        }
    }
    #[inline]
    pub fn num_of(&mut self, k: Sym) -> Option<f64> {
        let i = k.0 as usize;
        match self.mem.get(i) {
            Some(Some(v)) => return if Rt::plain(v) { Some(v.num()) } else { None },
            _ => {}
        }
        if let Some(Some(x)) = self.nums.get(i) {
            return Some(*x);
        }
        Some(self.get_val(k).num())
    }
    #[inline]
    pub fn val_of(&mut self, k: Sym) -> Option<Val> {
        let v = self.get_val(k);
        if Rt::plain(&v) { Some(v) } else { None }
    }
    #[inline]
    pub fn add_num(&mut self, k: Sym, d: f64) {
        let x = self.nums.get(k.0 as usize).copied().flatten().unwrap_or(0.0);
        self.nset(k, x + d);
    }
    pub fn interp(&mut self, s: &mut String, k: Sym) {
        let v = self.get_val(k);
        s.push_str(&v.text());
    }
    #[inline]
    pub fn lookup(&mut self, k: Sym) -> bool {
        if let (Some(None), Some(Some(x))) = (self.mem.get(k.0 as usize), self.nums.get(k.0 as usize)) {
            if self.repl.is_empty() {
                self.args.push(Val::N(*x));
                return false;
            }
        }
        let v = self.get_val(k);
        self.dispatch(v)
    }
    pub fn dispatch(&mut self, v: Val) -> bool {
        let c = match &v {
            Val::N(_) | Val::W(..) | Val::K(..) => None,
            Val::L(t) => cmd_of(t),
            Val::R(t) => cmd_of(t),
        };
        if let Some(c) = c {
            return self.cmd(c);
        }
        if !self.repl.is_empty() {
            let r = self.repl.get(&*v.text()).cloned();
            if let Some(r) = r {
                self.args.push(r);
                return false;
            }
        }
        self.args.push(v);
        false
    }
    fn joined(&self) -> String {
        let mut s = String::new();
        for (i, a) in self.args.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&a.text());
        }
        s
    }
    pub fn set_mem(&mut self, k: Sym) {
        let v = if self.args.len() == 1 {
            self.args.pop().unwrap()
        } else {
            Val::of(self.joined())
        };
        self.args.clear();
        self.mset(k, v);
    }
    pub fn set_num(&mut self, k: Sym) {
        let x = self.args.first().map(|v| v.num()).unwrap_or(0.0);
        self.args.clear();
        self.nset(k, x);
    }
    pub fn inc(&mut self, k: Sym) {
        let d = self.args.first().map(|v| v.num()).unwrap_or(1.0);
        self.args.clear();
        let x = self.nums.get(k.0 as usize).copied().flatten().unwrap_or(0.0);
        self.nset(k, x + d);
    }
    pub fn dec(&mut self, k: Sym) {
        let d = self.args.first().map(|v| v.num()).unwrap_or(1.0);
        self.args.clear();
        let x = self.nums.get(k.0 as usize).copied().flatten().unwrap_or(0.0);
        self.nset(k, x - d);
    }

    // scripts and units

    pub fn def_script(&mut self, k: Sym, hint: Option<usize>) {
        let body = self.joined().replace('|', "~");
        self.args.clear();
        let prog = self.prog;
        let sc = if let Some(h) = hint.filter(|&h| prog.chunks[h].0 == body) {
            Script::C(prog.chunks[h].1)
        } else if let Some(f) = prog.find(&body) {
            Script::C(f)
        } else {
            match parse(&body, &mut self.it) {
                Ok(o) => Script::U(Rc::new(o)),
                Err(e) => {
                    let m = format!("{}$: {}", self.it.name(k), e);
                    self.fail(m)
                }
            }
        };
        *grow(&mut self.scripts, k.0 as usize) = Some(sc);
    }

    pub fn call_once(&mut self, k: Sym) -> Flow {
        let sc = self.scripts.get(k.0 as usize).and_then(|x| x.clone());
        match sc {
            Some(Script::C(f)) => f(self),
            Some(Script::U(u)) => self.exec(&u),
            None => {
                let name = self.it.name(k).to_string();
                if self.prog.file(&name).is_some() || std::path::Path::new(&name).is_file() {
                    self.file_flow(&name)
                } else {
                    self.fail(format!("{}.: no script or file named '{}'", name, name))
                }
            }
        }
    }
    pub fn drive(&mut self, mut f: Flow) {
        while let Flow::Tail(k) = f {
            f = self.call_once(k);
        }
    }
    pub fn call(&mut self, k: Sym) {
        let f = self.call_once(k);
        self.drive(f);
    }
    fn count(&mut self, i: usize, w: &str) -> u64 {
        let v = &self.args[i];
        let x = match v {
            Val::N(x) => Some(*x),
            _ => v.text().trim().parse::<f64>().ok(),
        };
        match x {
            Some(x) if x.is_finite() && x >= 0.0 && x.fract() == 0.0 => x as u64,
            _ => {
                let m = format!("{}: repeat count must be a whole number >= 0, got '{}'", w, v.text());
                self.fail(m)
            }
        }
    }
    fn repeat(&mut self, k: Sym) {
        let w = format!("{}.", self.it.name(k));
        let n = self.count(0, &w);
        for _ in 0..n {
            self.args.clear();
            self.call(k);
        }
        self.args.clear();
    }
    pub fn call_site(&mut self, k: Sym) {
        if self.args.len() == 1 { self.repeat(k) } else { self.call(k) }
    }
    pub fn tail_site(&mut self, k: Sym) -> Flow {
        if self.args.len() == 1 {
            self.repeat(k);
            Flow::Done
        } else {
            Flow::Tail(k)
        }
    }

    pub fn src_flow(&mut self, src: &str) -> Flow {
        if let Some(f) = self.prog.find(src) {
            return f(self);
        }
        let u = match self.units.get(src) {
            Some(u) => u.clone(),
            None => {
                let u = match parse(src, &mut self.it) {
                    Ok(o) => Rc::new(o),
                    Err(e) => self.fail(format!("parse: {}", e)),
                };
                if self.units.len() > 4096 {
                    self.units.clear();
                }
                self.units.insert(src.to_string(), u.clone());
                u
            }
        };
        self.exec(&u)
    }
    pub fn run_src(&mut self, src: &str) {
        let f = self.src_flow(src);
        self.drive(f);
    }
    pub fn file_flow(&mut self, path: &str) -> Flow {
        if let Some(f) = self.prog.file(path) {
            return f(self);
        }
        match fs::read(path) {
            Ok(b) => {
                let s = String::from_utf8_lossy(&b).into_owned();
                self.src_flow(&s)
            }
            Err(e) => self.fail(format!("{}: {}", path, e)),
        }
    }
    pub fn run_file(&mut self, path: &str) {
        let f = self.file_flow(path);
        self.drive(f);
    }

    pub fn exec(&mut self, ops: &[Op]) -> Flow {
        let n = ops.len();
        for (i, op) in ops.iter().enumerate() {
            if let Some(f) = self.step(op, i + 1 == n) {
                return f;
            }
        }
        Flow::Done
    }

    fn step(&mut self, op: &Op, tail: bool) -> Option<Flow> {
        match op {
            Op::Push(v) => self.args.push(v.clone()),
            Op::Interp(ps) => {
                let mut s = String::new();
                for p in ps {
                    match p {
                        Part::T(t) => s.push_str(t),
                        Part::V(k) => self.interp(&mut s, *k),
                    }
                }
                self.push_string(s);
            }
            Op::Word(t, k, n) => self.word_rc(t, *k, *n),
            Op::Cmd(c) => {
                if self.cmd(*c) {
                    return Some(Flow::Done);
                }
            }
            Op::Lookup(k) => {
                if self.lookup(*k) {
                    return Some(Flow::Done);
                }
            }
            Op::Def(k) => self.def_script(*k, None),
            Op::SetMem(k) => self.set_mem(*k),
            Op::SetNum(k) => self.set_num(*k),
            Op::Inc(k) => self.inc(*k),
            Op::Dec(k) => self.dec(*k),
            Op::Call(k) => {
                if tail {
                    return Some(self.tail_site(*k));
                }
                self.call_site(*k);
            }
            Op::CallPass(k) => {
                if tail {
                    return Some(Flow::Tail(*k));
                }
                self.call(*k);
            }
            Op::Cond(t, inner) => {
                if self.ow == *t {
                    return self.step(inner, tail);
                }
            }
        }
        None
    }

    fn on_err(&mut self, i: usize, msg: String) {
        if self.args.len() > i {
            let script = self.t(i);
            self.args.clear();
            self.mset(S_ERR, Val::of(msg));
            self.run_src(&script);
        } else {
            self.fail(msg)
        }
    }

    fn worker(&self, i: f64) -> Option<usize> {
        let k = i as i64;
        if k >= 0 && (k as usize) < self.workers.len() && self.workers[k as usize].is_some() {
            Some(k as usize)
        } else {
            None
        }
    }

    fn list(&mut self, k: Sym) -> &mut Vec<Val> {
        grow(&mut self.lists, k.0 as usize).get_or_insert_with(Vec::new)
    }

    fn arith(&mut self, c: Cmd) {
        self.need(2, c);
        let (a, b) = (self.x(0), self.x(1));
        let r = match c {
            Cmd::Add => a + b,
            Cmd::Sub => a - b,
            Cmd::Mul => a * b,
            Cmd::Div => a / b,
            Cmd::Pow => a.powf(b),
            Cmd::Mod => a.rem_euclid(b),
            _ => unreachable!(),
        };
        let d = self.dest(2, S_EQ);
        self.nset(d, r);
    }

    fn cmp(&mut self, c: Cmd) {
        self.need(2, c);
        let (a, b) = (&self.args[0], &self.args[1]);
        self.ow = match c {
            Cmd::Eq => val_eq(a, b),
            Cmd::Ne => !val_eq(a, b),
            Cmd::AndNe => self.ow && !val_eq(a, b),
            Cmd::AndEq => self.ow && val_eq(a, b),
            Cmd::NumEq => a.num() == b.num(),
            Cmd::Lt => a.num() < b.num(),
            Cmd::Gt => a.num() > b.num(),
            Cmd::Le => a.num() <= b.num(),
            Cmd::Ge => a.num() >= b.num(),
            _ => unreachable!(),
        };
    }

    fn seed_of(&self) -> Option<(u64, u64)> {
        match self.args.len() {
            0 => None,
            1 => Some((self.x(0) as i64 as u64, 0)),
            _ => Some((self.x(0) as i64 as u64, self.x(1) as i64 as u64)),
        }
    }

    pub fn cmd(&mut self, c: Cmd) -> bool {
        use Cmd::*;
        match c {
            Print => {
                let mut s = String::from("\n");
                for a in &self.args {
                    s.push(' ');
                    s.push_str(&a.text());
                }
                let _ = self.out.write_all(s.as_bytes());
            }
            PrintRaw => {
                let mut s = String::new();
                for a in &self.args {
                    s.push_str(&a.text());
                }
                let _ = self.out.write_all(s.as_bytes());
            }
            Shell => {
                self.need(1, c);
                let k = self.t(0);
                self.flush();
                match shell(&k) {
                    Ok(o) => {
                        let s = String::from_utf8_lossy(&o.stdout).into_owned();
                        self.mset(S_R, Val::of(s));
                        self.nset(S_RC, o.status.code().unwrap_or(-1) as f64);
                    }
                    Err(e) => self.fail(format!("<: {}: {}", k, e)),
                }
            }
            ReadIn => {
                let d = self.dest(0, S_UNDER);
                self.flush();
                let mut line = String::new();
                let n = io::stdin().lock().read_line(&mut line).unwrap_or(0);
                while line.ends_with('\n') || line.ends_with('\r') {
                    line.pop();
                }
                self.mset(d, Val::of(line));
                self.mset(S_EOF, Val::N(if n == 0 { 1.0 } else { 0.0 }));
            }
            Write => {
                self.need(2, c);
                let (p, s) = (self.t(0), self.t(1));
                if let Err(e) = fs::write(&p, s) {
                    self.fail(format!("w: {}: {}", p, e))
                }
            }
            Append => {
                self.need(2, c);
                let (p, s) = (self.t(0), self.t(1));
                let r = OpenOptions::new().append(true).create(true).open(&p).and_then(|mut f| f.write_all(s.as_bytes()));
                if let Err(e) = r {
                    self.fail(format!("+w: {}: {}", p, e))
                }
            }
            Move => {
                self.need(2, c);
                let (a, b) = (self.t(0), self.t(1));
                if fs::rename(&a, &b).is_err() {
                    if let Err(e) = fs::copy(&a, &b).and_then(|_| fs::remove_file(&a)) {
                        self.fail(format!("mv: {} {}: {}", a, b, e))
                    }
                }
            }
            Read => {
                self.need(1, c);
                let p = self.t(0);
                let k = self.arg_sym(0);
                match fs::read(&p) {
                    Ok(b) => {
                        let s = String::from_utf8_lossy(&b).into_owned();
                        self.mset(k, Val::of(s));
                    }
                    Err(e) => self.fail(format!("r: {}: {}", p, e)),
                }
            }
            ReadAt => {
                self.need(4, c);
                let p = self.t(0);
                let (off, len) = (self.x(1) as u64, self.x(2) as usize);
                let d = self.arg_sym(3);
                let r = File::open(&p).and_then(|mut f| {
                    f.seek(SeekFrom::Start(off))?;
                    let mut b = Vec::with_capacity(len);
                    f.take(len as u64).read_to_end(&mut b)?;
                    Ok(b)
                });
                match r {
                    Ok(b) => {
                        let s = String::from_utf8_lossy(&b).into_owned();
                        self.mset(d, Val::of(s));
                    }
                    Err(e) => self.fail(format!("r@: {}: {}", p, e)),
                }
            }
            Match => {
                self.need(2, c);
                let (table, key, def) = (self.t(0), self.t(1), self.opt_t(2));
                self.args.clear();
                let mut ws = table.split_whitespace();
                let mut hit: Option<String> = None;
                while let (Some(k), Some(v)) = (ws.next(), ws.next()) {
                    if k == key {
                        hit = Some(v.to_string());
                        break;
                    }
                }
                match hit.or(def) {
                    Some(code) => self.run_src(&code),
                    None => {}
                }
                return false;
            }
            Repeat => {
                self.need(2, c);
                let n = self.count(0, "&&&");
                let k = self.arg_sym(1);
                for i in 0..n {
                    self.args.clear();
                    self.nset(S_N, i as f64);
                    self.call(k);
                }
            }
            Par => {
                let paths: Vec<String> = self.args.iter().map(|a| a.text().into_owned()).collect();
                self.args.clear();
                self.flush();
                let prog = self.prog;
                let outs: Vec<Option<String>> = thread::scope(|sc| {
                    let hs: Vec<_> = paths
                        .iter()
                        .map(|p| {
                            thread::Builder::new()
                                .stack_size(WORKER_STACK)
                                .spawn_scoped(sc, move || {
                                    let mut w = Rt::new(prog);
                                    w.run_file(p);
                                    w.flush();
                                    w.take_mem(S_OUT).unwrap_or_default()
                                })
                                .ok()
                        })
                        .collect();
                    hs.into_iter().map(|h| h.and_then(|h| h.join().ok())).collect()
                });
                for o in outs {
                    match o {
                        Some(o) => self.run_src(&o),
                        None => self.fail("***: worker failed".to_string()),
                    }
                }
                return false;
            }
            SetN => {
                self.need(2, c);
                let x = self.x(0);
                let k = self.arg_sym(1);
                self.nset(k, x);
            }
            Add | Sub | Mul | Div | Pow | Mod => self.arith(c),
            Not => self.ow = !self.ow,
            Eq | Ne | AndNe | AndEq | NumEq | Lt | Gt | Le | Ge => self.cmp(c),
            LPush => {
                self.need(2, c);
                let v = self.args[0].clone();
                let k = self.arg_sym(1);
                self.list(k).push(v);
            }
            LPop => {
                self.need(1, c);
                let k = self.arg_sym(0);
                let v = self.list(k).pop().unwrap_or(Val::L("404"));
                self.mset(S_DOLLAR, v);
            }
            LGet => {
                self.need(3, c);
                let k = self.arg_sym(0);
                let i = self.x(1) as i64;
                let d = self.arg_sym(2);
                let l = self.list(k);
                let i = if i < 0 { i + l.len() as i64 } else { i };
                if i >= 0 && (i as usize) < l.len() {
                    let v = l[i as usize].clone();
                    self.mset(d, v);
                }
            }
            LSet => {
                self.need(3, c);
                let v = self.args[0].clone();
                let k = self.arg_sym(1);
                let i = self.x(2) as i64;
                let l = self.list(k);
                let i = if i < 0 { i + l.len() as i64 } else { i };
                if i >= 0 && (i as usize) < l.len() {
                    l[i as usize] = v;
                }
            }
            LDel => {
                self.need(1, c);
                let k = self.arg_sym(0);
                if let Some(l) = self.lists.get_mut(k.0 as usize) {
                    *l = None;
                }
            }
            LLen => {
                self.need(1, c);
                let k = self.arg_sym(0);
                let n = self.lists.get(k.0 as usize).and_then(|l| l.as_ref()).map(|l| l.len()).unwrap_or(0);
                let d = self.dest(1, S_EQ);
                self.nset(d, n as f64);
            }
            Split => {
                self.need(2, c);
                let text = self.t(0);
                let k = self.arg_sym(1);
                let parts: Vec<Val> = match self.opt_t(2) {
                    None => text.split_whitespace().map(|s| Val::of(s.to_string())).collect(),
                    Some(sep) if sep.is_empty() => text.chars().map(|ch| Val::of(ch.to_string())).collect(),
                    Some(sep) => text.split(sep.as_str()).map(|s| Val::of(s.to_string())).collect(),
                };
                *grow(&mut self.lists, k.0 as usize) = Some(parts);
            }
            Join => {
                self.need(2, c);
                let k = self.arg_sym(0);
                let d = self.arg_sym(1);
                let sep = self.opt_t(2).unwrap_or_else(|| " ".to_string());
                let s = match self.lists.get(k.0 as usize).and_then(|l| l.as_ref()) {
                    Some(l) => l.iter().map(|v| v.text().into_owned()).collect::<Vec<_>>().join(&sep),
                    None => String::new(),
                };
                self.mset(d, Val::of(s));
            }
            SLen => {
                self.need(1, c);
                let n = self.args[0].text().chars().count();
                let d = self.dest(1, S_EQ);
                self.nset(d, n as f64);
            }
            Open => {
                self.need(3, c);
                let (p, mode) = (self.t(0), self.t(2));
                let k = self.arg_sym(1);
                match open_mode(&p, &mode) {
                    Ok(f) => {
                        self.files.insert(k.0, FileH { f, eof: false });
                    }
                    Err(e) => {
                        self.on_err(3, format!("f: {}: {}", p, e));
                        return false;
                    }
                }
            }
            FWriteAt => {
                self.need(3, c);
                let k = self.arg_sym(0);
                let off = self.x(1) as u64;
                let key = self.arg_sym(2);
                let data = match self.mem.get(key.0 as usize) {
                    Some(Some(v)) => v.text().into_owned(),
                    _ => String::new(),
                };
                let r = match self.files.get_mut(&k.0) {
                    Some(h) => h.f.seek(SeekFrom::Start(off)).and_then(|_| h.f.write_all(data.as_bytes())),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "no open file")),
                };
                if let Err(e) = r {
                    let m = format!("+f: {}: {}", self.it.name(k), e);
                    self.on_err(3, m);
                    return false;
                }
            }
            FReadAt => {
                self.need(4, c);
                let k = self.arg_sym(0);
                let (off, len) = (self.x(1) as u64, self.x(2) as usize);
                let d = self.arg_sym(3);
                let r = match self.files.get_mut(&k.0) {
                    Some(h) => {
                        let r = h.f.seek(SeekFrom::Start(off)).and_then(|_| {
                            let mut b = Vec::with_capacity(len);
                            (&mut h.f).take(len as u64).read_to_end(&mut b)?;
                            Ok(b)
                        });
                        if let Ok(b) = &r {
                            h.eof = b.len() < len;
                        }
                        r
                    }
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "no open file")),
                };
                match r {
                    Ok(b) => {
                        let s = String::from_utf8_lossy(&b).into_owned();
                        self.mset(d, Val::of(s));
                    }
                    Err(e) => {
                        let m = format!("@f: {}: {}", self.it.name(k), e);
                        self.fail(m)
                    }
                }
            }
            FClose => {
                self.need(1, c);
                let k = self.arg_sym(0);
                self.files.remove(&k.0);
            }
            FWriteLn => {
                self.need(2, c);
                let k = self.arg_sym(0);
                let mut line = self.t(1);
                line.push('\n');
                let r = match self.files.get_mut(&k.0) {
                    Some(h) => h.f.write_all(line.as_bytes()),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "no open file")),
                };
                if let Err(e) = r {
                    let m = format!("f_: {}: {}", self.it.name(k), e);
                    self.on_err(2, m);
                    return false;
                }
            }
            FReadLn => {
                self.need(2, c);
                let k = self.arg_sym(0);
                let d = self.arg_sym(1);
                let r = match self.files.get_mut(&k.0) {
                    Some(h) => read_line_h(h),
                    None => Err(io::Error::new(io::ErrorKind::NotFound, "no open file")),
                };
                match r {
                    Ok(s) => self.mset(d, Val::of(s)),
                    Err(e) => {
                        let m = format!("_f: {}: {}", self.it.name(k), e);
                        self.fail(m)
                    }
                }
            }
            Eof => {
                self.need(1, c);
                let k = self.arg_sym(0);
                let e = self.files.get(&k.0).map(|h| h.eof).unwrap_or(true);
                self.mset(S_EOF, Val::N(if e { 1.0 } else { 0.0 }));
            }
            Spawn => {
                let paths: Vec<String> = self.args.iter().map(|a| a.text().into_owned()).collect();
                for p in paths {
                    let (to_tx, to_rx) = channel::<String>();
                    let (from_tx, from_rx) = channel::<String>();
                    let prog = self.prog;
                    let r = thread::Builder::new().stack_size(WORKER_STACK).spawn(move || {
                        let mut w = Rt::new(prog);
                        w.run_file(&p);
                        w.flush();
                        while let Ok(m) = to_rx.recv() {
                            if m == "__stop__" {
                                break;
                            }
                            w.run_src(&m);
                            w.flush();
                            if let Some(o) = w.take_mem(S_OUT) {
                                if from_tx.send(o).is_err() {
                                    break;
                                }
                            }
                        }
                        w.flush();
                    });
                    if let Err(e) = r {
                        self.fail(format!("t: {}", e))
                    }
                    self.workers.push(Some(Worker { tx: to_tx, rx: from_rx }));
                    let k = (self.workers.len() - 1) as f64;
                    self.nset(S_THREADS, k);
                }
            }
            Kill => {
                self.need(1, c);
                match self.worker(self.x(0)) {
                    Some(k) => self.workers[k] = None,
                    None => self.mset(S_ERR, Val::L("Invalid thread index")),
                }
            }
            Send => {
                self.need(2, c);
                let m = self.t(0);
                match self.worker(self.x(1)) {
                    Some(k) => {
                        if self.workers[k].as_ref().unwrap().tx.send(m).is_err() {
                            self.mset(S_ERR, Val::L("thread closed"));
                        }
                    }
                    None => self.mset(S_ERR, Val::L("Invalid thread index")),
                }
            }
            Recv => {
                self.need(1, c);
                let i = self.x(0);
                self.args.clear();
                match self.worker(i) {
                    Some(k) => {
                        self.flush();
                        let m = self.workers[k].as_ref().unwrap().rx.recv();
                        match m {
                            Ok(m) => self.run_src(&m),
                            Err(_) => self.mset(S_ERR, Val::L("thread closed")),
                        }
                    }
                    None => self.mset(S_ERR, Val::L("Invalid thread index")),
                }
                return false;
            }
            PrngN => {
                if let Some((a, b)) = self.seed_of() {
                    self.rng = Rng::seeded(a, b);
                }
                let d = self.dest(2, S_RN);
                let x = self.rng.f64();
                self.nset(d, x);
            }
            PrngS => {
                const AZ: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
                if let Some((a, b)) = self.seed_of() {
                    self.rng = Rng::seeded(a, b);
                }
                let d = self.dest(2, S_RN);
                let s: String = (0..32).map(|_| AZ[(self.rng.next() % 52) as usize] as char).collect();
                self.mset(d, Val::of(s));
            }
            Replace => {
                self.need(2, c);
                let k: Rc<str> = Rc::from(self.t(1));
                let v = self.args[0].clone();
                self.repl.insert(k, v);
            }
            Comment => {}
            Ret => return true,
            Exit => {
                let code = self.args.first().map(|v| v.num() as i32).unwrap_or(0);
                self.flush();
                std::process::exit(code)
            }
        }
        self.args.clear();
        false
    }
}

pub enum Entry {
    Chunk(usize),
    File(String),
}

pub fn s_main(prog: &'static Program, entry: Entry, argv: Vec<String>) -> ! {
    let h = thread::Builder::new()
        .stack_size(MAIN_STACK)
        .spawn(move || {
            let mut rt = Rt::new(prog);
            rt.set_argv(argv);
            let f = match entry {
                Entry::Chunk(i) => (prog.chunks[i].1)(&mut rt),
                Entry::File(p) => rt.file_flow(&p),
            };
            rt.drive(f);
            rt.flush();
        })
        .expect("spawn main thread");
    let code = if h.join().is_ok() { 0 } else { 101 };
    std::process::exit(code)
}
