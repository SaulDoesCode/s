// s runtime for the browser. Semantics follow src/rt.rs.
var SRT = (function () {
  "use strict";

  class SErr extends Error {}
  class SExit extends Error {
    constructor(code) { super("exit " + code); this.code = code; }
  }

  const enc = new TextEncoder();
  const dec = new TextDecoder("utf-8", { fatal: false });

  const CMDS = {
    ">": "Print", ">_": "PrintRaw", "<": "Shell", "<_": "ReadIn",
    "w": "Write", "+w": "Append", "mv": "Move", "r": "Read", "r@": "ReadAt",
    "?)": "Match", "&&&": "Repeat", "***": "Par",
    "n": "SetN", "+": "Add", "-": "Sub", "*": "Mul", "/": "Div", "^": "Pow", "%": "Mod",
    "!!": "Not", "=": "Eq", "!=": "Ne", ";": "AndNe", "&": "AndEq",
    "==": "NumEq", "<<": "Lt", ">>": "Gt", "<<=": "Le", ">>=": "Ge",
    "l": "LPush", "-l": "LPop", "[l]": "LGet", "l=": "LSet", "l-": "LDel",
    "#l": "LLen", "/l": "Split", "l/": "Join", "#s": "SLen",
    "f": "Open", "+f": "FWriteAt", "@f": "FReadAt", "f-": "FClose",
    "f_": "FWriteLn", "_f": "FReadLn", "?eof": "Eof",
    "t": "Spawn", "-t": "Kill", "<t": "Send", ">t": "Recv",
    "#prng": "PrngN", "`prng": "PrngS",
    "><": "Replace", "//": "Comment", "<-": "Ret", "<--": "Exit",
  };
  const cmdOf = (w) => (Object.prototype.hasOwnProperty.call(CMDS, w) ? CMDS[w] : null);
  const SUFFIX = "?!~$`#;:.";

  const NUM_RE = /^[+-]?(?:(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?|inf|infinity|nan)$/i;

  function strictNum(s) {
    if (!NUM_RE.test(s)) return null;
    const l = s.toLowerCase().replace(/^[+-]/, "");
    const neg = s[0] === "-";
    if (l === "inf" || l === "infinity") return neg ? -Infinity : Infinity;
    if (l === "nan") return NaN;
    return parseFloat(s);
  }
  function parseNum(s) {
    const x = strictNum(s.trim());
    return x === null ? 0 : x;
  }
  function fmtNum(x) {
    if (x === 0) return "0";
    if (Number.isNaN(x)) return "NaN";
    if (x === Infinity) return "inf";
    if (x === -Infinity) return "-inf";
    const s = String(x);
    const e = s.indexOf("e");
    if (e < 0) return s;
    let m = s.slice(0, e), ex = parseInt(s.slice(e + 1), 10);
    let neg = false;
    if (m[0] === "-") { neg = true; m = m.slice(1); }
    let [ip, fp = ""] = m.split(".");
    let digits = ip + fp, point = ip.length + ex;
    let out;
    if (point <= 0) out = "0." + "0".repeat(-point) + digits;
    else if (point >= digits.length) out = digits + "0".repeat(point - digits.length);
    else out = digits.slice(0, point) + "." + digits.slice(point);
    return (neg ? "-" : "") + out;
  }
  function canon(s) {
    const x = strictNum(s);
    if (x === null) return null;
    return fmtNum(x) === s ? x : null;
  }
  const text = (v) => (typeof v === "number" ? fmtNum(v) : v);
  const num = (v) => (typeof v === "number" ? v : parseNum(v));
  function valEq(a, b) {
    if (typeof a === "number" && typeof b === "number" && !Number.isNaN(a) && !Number.isNaN(b)) return a === b;
    return text(a) === text(b);
  }
  function toI64(x) {
    if (Number.isNaN(x)) return 0;
    x = Math.trunc(x);
    if (x > 9223372036854775807) return 9223372036854775807;
    if (x < -9223372036854775808) return -9223372036854775808;
    return x;
  }
  const toU = (x) => Math.max(0, toI64(x));
  const isWs = (c) => c === " " || c === "\n" || c === "\t" || c === "\r";

  // parser

  function classify(w) {
    const c = cmdOf(w);
    if (c) return { k: "cmd", c };
    const cs = Array.from(w);
    const last = cs[cs.length - 1];
    const stem = cs.slice(0, -1).join("");
    if (stem.length > 0 && SUFFIX.includes(last)) {
      switch (last) {
        case "?": return { k: "cond", t: true, op: classify(stem) };
        case "!": return { k: "cond", t: false, op: classify(stem) };
        case "~": return { k: "lookup", s: stem };
        case "$": return { k: "def", s: stem };
        case "`": return { k: "setmem", s: stem };
        case "#": return { k: "setnum", s: stem };
        case ";": return { k: "inc", s: stem };
        case ":": return { k: "dec", s: stem };
        case ".":
          if (stem.length > 1 && stem.endsWith(".")) return { k: "callpass", s: stem.slice(0, -1) };
          return { k: "call", s: stem };
      }
    }
    return { k: "word", t: w };
  }

  function brace(cs, i) {
    const n = cs.length;
    let depth = 1, j = i + 1, parts = [], cur = "";
    while (j < n) {
      const c = cs[j];
      if (c === "{") { depth++; cur += c; j++; }
      else if (c === "}") {
        depth--;
        if (depth === 0) { if (cur) parts.push({ t: cur }); return [parts, j + 1]; }
        cur += c; j++;
      } else if (c === "~" && depth === 1) {
        let k = j + 1;
        while (k < n && cs[k] !== "~" && cs[k] !== "{" && cs[k] !== "}" && !isWs(cs[k])) k++;
        if (k < n && cs[k] === "~") {
          if (k === j + 1) cur += "~";
          else {
            if (cur) { parts.push({ t: cur }); cur = ""; }
            parts.push({ v: cs.slice(j + 1, k).join("") });
          }
          j = k + 1;
        } else { cur += "~"; j++; }
      } else { cur += c; j++; }
    }
    throw new SErr("unclosed {");
  }

  function litOp(prefix, parts) {
    const ps = [];
    for (const p of [{ t: prefix }, ...parts]) {
      if (p.t !== undefined) {
        if (p.t === "") continue;
        const last = ps[ps.length - 1];
        if (last && last.t !== undefined) last.t += p.t;
        else ps.push({ t: p.t });
      } else ps.push(p);
    }
    if (ps.every((p) => p.t !== undefined)) return { k: "push", v: ps.length ? ps[0].t : "" };
    return { k: "interp", parts: ps };
  }

  function parse(src) {
    const cs = Array.from(src);
    const n = cs.length;
    const ops = [];
    let i = 0;
    while (i < n) {
      if (isWs(cs[i])) { i++; continue; }
      let word = "", done = false;
      while (i < n && !isWs(cs[i])) {
        const c = cs[i];
        if (c === "{") {
          const [parts, j] = brace(cs, i);
          ops.push(litOp(word, parts));
          word = ""; i = j; done = true; break;
        } else if (c === "'") {
          let j = i + 1;
          while (j < n && cs[j] !== "'") j++;
          if (j >= n) throw new SErr("unclosed '");
          ops.push({ k: "push", v: word + cs.slice(i + 1, j).join("") });
          word = ""; i = j + 1; done = true; break;
        } else { word += c; i++; }
      }
      if (!done && word) ops.push(classify(word));
    }
    return ops;
  }

  // prng: xoshiro256** seeded through splitmix64, identical to rt.rs

  const M64 = (1n << 64n) - 1n;
  const rotl = (x, k) => ((x << BigInt(k)) | (x >> BigInt(64 - k))) & M64;
  function splitmix(st) {
    st.z = (st.z + 0x9E3779B97F4A7C15n) & M64;
    let x = st.z;
    x = ((x ^ (x >> 30n)) * 0xBF58476D1CE4E5B9n) & M64;
    x = ((x ^ (x >> 27n)) * 0x94D049BB133111EBn) & M64;
    return x ^ (x >> 31n);
  }
  class Rng {
    constructor(a, b) {
      const st = { z: (a ^ rotl(b, 32)) & M64 };
      this.s = [splitmix(st), splitmix(st), splitmix(st), splitmix(st)];
    }
    next() {
      const s = this.s;
      const r = (rotl((s[1] * 5n) & M64, 7) * 9n) & M64;
      const t = (s[1] << 17n) & M64;
      s[2] ^= s[0]; s[3] ^= s[1]; s[1] ^= s[2]; s[0] ^= s[3]; s[2] ^= t;
      s[3] = rotl(s[3], 45);
      return r;
    }
    f64() { return Number(this.next() >> 11n) / 9007199254740992; }
  }
  function osU64() {
    const a = new BigUint64Array(1);
    crypto.getRandomValues(a);
    return a[0];
  }
  const seedU = (x) => BigInt.asUintN(64, BigInt(toI64(x)));

  // shell emulation over the virtual file system

  function shWords(line) {
    const out = [];
    let cur = null, q = null;
    for (const c of line) {
      if (q) { if (c === q) q = null; else cur += c; continue; }
      if (c === "'" || c === '"') { q = c; if (cur === null) cur = ""; continue; }
      if (isWs(c)) { if (cur !== null) { out.push(cur); cur = null; } continue; }
      cur = (cur || "") + c;
    }
    if (cur !== null) out.push(cur);
    return out;
  }
  function shell(host, cmd) {
    let stdout = "", code = 0;
    for (const part of cmd.split(/;|\n/)) {
      const w = shWords(part);
      if (!w.length) continue;
      const [c, ...a] = w;
      const p = (s) => s.replace(/^\.\//, "");
      switch (c) {
        case "echo": {
          const nl = a[0] !== "-n";
          stdout += (nl ? a : a.slice(1)).join(" ") + (nl ? "\n" : ""); code = 0; break;
        }
        case "cat":
          code = 0;
          for (const f of a) {
            const b = host.vfs.get(p(f));
            if (b) stdout += dec.decode(b);
            else { host.err("cat: " + f + ": No such file or directory\n"); code = 1; }
          }
          break;
        case "ls": stdout += [...host.vfs.keys()].sort().join("\n") + "\n"; code = 0; break;
        case "rm":
          code = 0;
          for (const f of a) if (!f.startsWith("-") && !host.vfs.delete(p(f))) { host.err("rm: cannot remove '" + f + "': No such file or directory\n"); code = 1; }
          host.changed();
          break;
        case "cp": case "mv": {
          const [x, y] = a.map(p);
          const b = host.vfs.get(x);
          if (!b) { host.err(c + ": cannot stat '" + x + "': No such file or directory\n"); code = 1; break; }
          host.vfs.set(y, b.slice());
          if (c === "mv") host.vfs.delete(x);
          host.changed(); code = 0; break;
        }
        case "pwd": stdout += "/\n"; code = 0; break;
        case "true": code = 0; break;
        case "false": code = 1; break;
        case "exit": return { stdout, code: a.length ? (parseInt(a[0], 10) & 255) : code };
        default: host.err("sh: 1: " + c + ": not found\n"); code = 127;
      }
    }
    return { stdout, code };
  }

  const OS2 = "No such file or directory (os error 2)";
  const OS9 = "Bad file descriptor (os error 9)";
  const normPath = (p) => p.replace(/^\.\//, "");

  // interpreter

  class Rt {
    constructor(host, argv) {
      this.host = host;
      this.args = [];
      this.ow = false;
      this.mem = new Map();
      this.nums = new Map();
      this.scripts = new Map();
      this.lists = new Map();
      this.repl = new Map();
      this.files = new Map();
      this.workers = [];
      this.units = new Map();
      this.rng = new Rng(osU64(), osU64());
      if (argv) {
        this.lists.set("args", argv.slice());
        this.nums.set("argc", argv.length);
      }
    }

    fail(m) { throw new SErr(m); }
    out(s) { this.host.out(s); }

    need(n, c) {
      if (this.args.length < n) this.fail(`${c}: needs ${n} args, stack has ${this.args.length}`);
    }
    t(i) { return text(this.args[i]); }
    optT(i) { return i < this.args.length ? text(this.args[i]) : null; }
    x(i) { return num(this.args[i]); }
    dest(i, d) { return this.args.length > i ? this.t(i) : d; }

    getVal(k) {
      if (this.mem.has(k)) return this.mem.get(k);
      if (this.nums.has(k)) return this.nums.get(k);
      if (k === "?") return osU64().toString();
      return "404";
    }
    lookup(k) { return this.dispatch(this.getVal(k)); }
    dispatch(v) {
      if (typeof v === "string") {
        const c = cmdOf(v);
        if (c) return this.cmd(c, v);
      }
      if (this.repl.size) {
        const r = this.repl.get(text(v));
        if (r !== undefined) { this.args.push(r); return false; }
      }
      this.args.push(v);
      return false;
    }
    word(w) {
      if (this.repl.size) {
        const r = this.repl.get(w);
        if (r !== undefined) { this.args.push(r); return; }
      }
      this.args.push(w);
    }
    joined() { return this.args.map(text).join(" "); }
    setMem(k) {
      const v = this.args.length === 1 ? this.args[0] : this.joined();
      this.args = [];
      this.mem.set(k, v);
    }
    setNum(k) {
      const x = this.args.length ? num(this.args[0]) : 0;
      this.args = [];
      this.nums.set(k, x);
    }
    incBy(k, sign) {
      const d = this.args.length ? num(this.args[0]) : 1;
      this.args = [];
      this.nums.set(k, (this.nums.has(k) ? this.nums.get(k) : 0) + sign * d);
    }
    takeMem(k) {
      if (!this.mem.has(k)) return null;
      const v = this.mem.get(k);
      this.mem.delete(k);
      return text(v);
    }

    defScript(k) {
      const body = this.joined().replace(/\|/g, "~");
      this.args = [];
      let ops;
      try { ops = parse(body); } catch (e) { if (e instanceof SErr) this.fail(`${k}$: ${e.message}`); throw e; }
      this.scripts.set(k, ops);
    }
    callOnce(k) {
      const sc = this.scripts.get(k);
      if (sc) return this.exec(sc);
      if (this.host.vfs.has(normPath(k))) return this.fileFlow(k);
      this.fail(`${k}.: no script or file named '${k}'`);
    }
    drive(f) { while (f !== null) f = this.callOnce(f); }
    call(k) { this.drive(this.callOnce(k)); }
    count(i, w) {
      const v = this.args[i];
      const x = typeof v === "number" ? v : strictNum(v.trim());
      if (x === null || !Number.isFinite(x) || x < 0 || x !== Math.trunc(x)) {
        this.fail(`${w}: repeat count must be a whole number >= 0, got '${text(v)}'`);
      }
      return x;
    }
    repeat(k) {
      const n = this.count(0, k + ".");
      for (let i = 0; i < n; i++) { this.args = []; this.call(k); }
      this.args = [];
    }
    callSite(k) { if (this.args.length === 1) this.repeat(k); else this.call(k); }
    tailSite(k) { if (this.args.length === 1) { this.repeat(k); return null; } return k; }

    srcFlow(src) {
      let u = this.units.get(src);
      if (!u) {
        try { u = parse(src); } catch (e) { if (e instanceof SErr) this.fail("parse: " + e.message); throw e; }
        if (this.units.size > 4096) this.units.clear();
        this.units.set(src, u);
      }
      return this.exec(u);
    }
    runSrc(src) { this.drive(this.srcFlow(src)); }
    fileFlow(path) {
      const b = this.host.vfs.get(normPath(path));
      if (!b) this.fail(`${path}: ${OS2}`);
      return this.srcFlow(dec.decode(b));
    }
    runFile(path) { this.drive(this.fileFlow(path)); }

    exec(ops) {
      const n = ops.length;
      for (let i = 0; i < n; i++) {
        const f = this.step(ops[i], i + 1 === n);
        if (f !== undefined) return f;
      }
      return null;
    }
    // returns undefined to continue, null to end the unit, or a script name for a tail call
    step(op, tail) {
      this.host.tick();
      switch (op.k) {
        case "push": this.args.push(op.v); break;
        case "interp": {
          let s = "";
          for (const p of op.parts) s += p.t !== undefined ? p.t : text(this.getVal(p.v));
          this.args.push(s);
          break;
        }
        case "word": this.word(op.t); break;
        case "cmd": if (this.cmd(op.c, null)) return null; break;
        case "lookup": if (this.lookup(op.s)) return null; break;
        case "def": this.defScript(op.s); break;
        case "setmem": this.setMem(op.s); break;
        case "setnum": this.setNum(op.s); break;
        case "inc": this.incBy(op.s, 1); break;
        case "dec": this.incBy(op.s, -1); break;
        case "call":
          if (tail) return this.tailSite(op.s);
          this.callSite(op.s); break;
        case "callpass":
          if (tail) return op.s;
          this.call(op.s); break;
        case "cond":
          if (this.ow === op.t) return this.step(op.op, tail);
          break;
      }
      return undefined;
    }

    onErr(i, msg) {
      if (this.args.length > i) {
        const code = this.t(i);
        this.args = [];
        this.mem.set("err", msg);
        this.runSrc(code);
      } else this.fail(msg);
    }
    worker(x) {
      const k = toI64(x);
      return k >= 0 && k < this.workers.length && this.workers[k] ? k : -1;
    }
    list(k) {
      let l = this.lists.get(k);
      if (!l) { l = []; this.lists.set(k, l); }
      return l;
    }
    fileBytes(path) { return this.host.vfs.get(path); }

    cmd(c, w) {
      const W = Object.keys(CMDS).find((k) => CMDS[k] === c);
      const a = this.args;
      switch (c) {
        case "Print": this.out("\n" + a.map((v) => " " + text(v)).join("")); break;
        case "PrintRaw": this.out(a.map(text).join("")); break;
        case "Shell": {
          this.need(1, W);
          const r = shell(this.host, this.t(0));
          this.mem.set("r", r.stdout);
          this.nums.set("rc", r.code);
          break;
        }
        case "ReadIn": {
          const d = this.dest(0, "_");
          const h = this.host;
          if (h.stdinPos < h.stdin.length) {
            this.mem.set(d, h.stdin[h.stdinPos++].replace(/[\r\n]+$/, ""));
            this.mem.set("eof", 0);
          } else {
            this.mem.set(d, "");
            this.mem.set("eof", 1);
          }
          break;
        }
        case "Write": {
          this.need(2, W);
          this.host.vfs.set(normPath(this.t(0)), enc.encode(this.t(1)));
          this.host.changed();
          break;
        }
        case "Append": {
          this.need(2, W);
          const p = normPath(this.t(0));
          const old = this.host.vfs.get(p) || new Uint8Array(0);
          const add = enc.encode(this.t(1));
          const b = new Uint8Array(old.length + add.length);
          b.set(old); b.set(add, old.length);
          this.host.vfs.set(p, b);
          this.host.changed();
          break;
        }
        case "Move": {
          this.need(2, W);
          const [x, y] = [normPath(this.t(0)), normPath(this.t(1))];
          const b = this.host.vfs.get(x);
          if (!b) this.fail(`mv: ${this.t(0)} ${this.t(1)}: ${OS2}`);
          this.host.vfs.delete(x);
          this.host.vfs.set(y, b);
          this.host.changed();
          break;
        }
        case "Read": {
          this.need(1, W);
          const p = this.t(0);
          const b = this.host.vfs.get(normPath(p));
          if (!b) this.fail(`r: ${p}: ${OS2}`);
          this.mem.set(p, dec.decode(b));
          break;
        }
        case "ReadAt": {
          this.need(4, W);
          const p = this.t(0);
          const b = this.host.vfs.get(normPath(p));
          if (!b) this.fail(`r@: ${p}: ${OS2}`);
          const off = toU(this.x(1)), len = toU(this.x(2));
          this.mem.set(this.t(3), dec.decode(b.subarray(Math.min(off, b.length), Math.min(off + len, b.length))));
          break;
        }
        case "Match": {
          this.need(2, W);
          const table = this.t(0), key = this.t(1), def = this.optT(2);
          this.args = [];
          const ws = table.split(/\s+/).filter((s) => s);
          let hit = null;
          for (let i = 0; i + 1 < ws.length; i += 2) if (ws[i] === key) { hit = ws[i + 1]; break; }
          const code = hit !== null ? hit : def;
          if (code !== null) this.runSrc(code);
          return false;
        }
        case "Repeat": {
          this.need(2, W);
          const n = this.count(0, "&&&");
          const k = this.t(1);
          for (let i = 0; i < n; i++) {
            this.args = [];
            this.nums.set("n", i);
            this.call(k);
          }
          break;
        }
        case "Par": {
          const paths = a.map(text);
          this.args = [];
          const outs = paths.map((p) => {
            const w = new Rt(this.host, null);
            w.runFile(p);
            const o = w.takeMem("out");
            return o === null ? "" : o;
          });
          for (const o of outs) this.runSrc(o);
          return false;
        }
        case "SetN": this.need(2, W); this.nums.set(this.t(1), this.x(0)); break;
        case "Add": case "Sub": case "Mul": case "Div": case "Pow": case "Mod": {
          this.need(2, W);
          const x = this.x(0), y = this.x(1);
          let r;
          switch (c) {
            case "Add": r = x + y; break;
            case "Sub": r = x - y; break;
            case "Mul": r = x * y; break;
            case "Div": r = x / y; break;
            case "Pow": r = Math.pow(x, y); break;
            case "Mod": r = x % y; if (r < 0) r += Math.abs(y); break;
          }
          this.nums.set(this.dest(2, "="), r);
          break;
        }
        case "Not": this.ow = !this.ow; break;
        case "Eq": case "Ne": case "AndNe": case "AndEq": case "NumEq": case "Lt": case "Gt": case "Le": case "Ge": {
          this.need(2, W);
          const p = a[0], q = a[1];
          switch (c) {
            case "Eq": this.ow = valEq(p, q); break;
            case "Ne": this.ow = !valEq(p, q); break;
            case "AndNe": this.ow = this.ow && !valEq(p, q); break;
            case "AndEq": this.ow = this.ow && valEq(p, q); break;
            case "NumEq": this.ow = num(p) === num(q); break;
            case "Lt": this.ow = num(p) < num(q); break;
            case "Gt": this.ow = num(p) > num(q); break;
            case "Le": this.ow = num(p) <= num(q); break;
            case "Ge": this.ow = num(p) >= num(q); break;
          }
          break;
        }
        case "LPush": this.need(2, W); this.list(this.t(1)).push(a[0]); break;
        case "LPop": {
          this.need(1, W);
          const l = this.list(this.t(0));
          this.mem.set("$", l.length ? l.pop() : "404");
          break;
        }
        case "LGet": {
          this.need(3, W);
          const l = this.list(this.t(0));
          let i = toI64(this.x(1));
          if (i < 0) i += l.length;
          if (i >= 0 && i < l.length) this.mem.set(this.t(2), l[i]);
          break;
        }
        case "LSet": {
          this.need(3, W);
          const l = this.list(this.t(1));
          let i = toI64(this.x(2));
          if (i < 0) i += l.length;
          if (i >= 0 && i < l.length) l[i] = a[0];
          break;
        }
        case "LDel": this.need(1, W); this.lists.delete(this.t(0)); break;
        case "LLen": {
          this.need(1, W);
          const l = this.lists.get(this.t(0));
          this.nums.set(this.dest(1, "="), l ? l.length : 0);
          break;
        }
        case "Split": {
          this.need(2, W);
          const s = this.t(0), sep = this.optT(2);
          let parts;
          if (sep === null) parts = s.split(/\s+/).filter((x) => x);
          else if (sep === "") parts = Array.from(s);
          else parts = s.split(sep);
          this.lists.set(this.t(1), parts);
          break;
        }
        case "Join": {
          this.need(2, W);
          const l = this.lists.get(this.t(0));
          const sep = this.optT(2);
          this.mem.set(this.t(1), l ? l.map(text).join(sep === null ? " " : sep) : "");
          break;
        }
        case "SLen": this.need(1, W); this.nums.set(this.dest(1, "="), Array.from(this.t(0)).length); break;
        case "Open": {
          this.need(3, W);
          const p = normPath(this.t(0)), name = this.t(1), mode = this.t(2).replace(/b/g, "");
          const vfs = this.host.vfs;
          const modes = {
            "r": [1, 0, 0, 0, 0], "w": [0, 1, 1, 1, 0], "a": [0, 1, 1, 0, 1],
            "r+": [1, 1, 0, 0, 0], "w+": [1, 1, 1, 1, 0], "a+": [1, 1, 1, 0, 1],
          };
          const m = modes[mode];
          let err = null;
          if (!m) err = `bad mode '${this.t(2)}'`;
          else if (!vfs.has(p) && !m[2]) err = OS2;
          if (err) { this.onErr(3, `f: ${this.t(0)}: ${err}`); return false; }
          if (!vfs.has(p) || m[3]) { vfs.set(p, new Uint8Array(0)); this.host.changed(); }
          this.files.set(name, { path: p, pos: 0, eof: false, rd: !!m[0], wr: !!m[1], app: !!m[4] });
          break;
        }
        case "FWriteAt": {
          this.need(3, W);
          const name = this.t(0), off = toU(this.x(1)), key = this.t(2);
          const h = this.files.get(name);
          const err = !h ? "no open file" : !h.wr ? OS9 : null;
          if (err) { this.onErr(3, `+f: ${name}: ${err}`); return false; }
          const data = enc.encode(this.mem.has(key) ? text(this.mem.get(key)) : "");
          this.writeAt(h, h.app ? null : off, data);
          break;
        }
        case "FReadAt": {
          this.need(4, W);
          const name = this.t(0), off = toU(this.x(1)), len = toU(this.x(2));
          const h = this.files.get(name);
          if (!h) this.fail(`@f: ${name}: no open file`);
          if (!h.rd) this.fail(`@f: ${name}: ${OS9}`);
          const b = this.host.vfs.get(h.path) || new Uint8Array(0);
          const got = b.subarray(Math.min(off, b.length), Math.min(off + len, b.length));
          h.pos = off + got.length;
          h.eof = got.length < len;
          this.mem.set(this.t(3), dec.decode(got));
          break;
        }
        case "FClose": this.need(1, W); this.files.delete(this.t(0)); break;
        case "FWriteLn": {
          this.need(2, W);
          const name = this.t(0);
          const h = this.files.get(name);
          const err = !h ? "no open file" : !h.wr ? OS9 : null;
          if (err) { this.onErr(2, `f_: ${name}: ${err}`); return false; }
          this.writeAt(h, h.app ? null : h.pos, enc.encode(this.t(1) + "\n"));
          break;
        }
        case "FReadLn": {
          this.need(2, W);
          const name = this.t(0);
          const h = this.files.get(name);
          if (!h) this.fail(`_f: ${name}: no open file`);
          if (!h.rd) this.fail(`_f: ${name}: ${OS9}`);
          const b = this.host.vfs.get(h.path) || new Uint8Array(0);
          if (h.pos >= b.length) { h.eof = true; this.mem.set(this.t(1), ""); break; }
          let e = b.indexOf(10, h.pos);
          const end = e < 0 ? b.length : e;
          let line = b.subarray(h.pos, end);
          if (line.length && line[line.length - 1] === 13) line = line.subarray(0, line.length - 1);
          h.pos = e < 0 ? b.length : e + 1;
          h.eof = false;
          this.mem.set(this.t(1), dec.decode(line));
          break;
        }
        case "Eof": {
          this.need(1, W);
          const h = this.files.get(this.t(0));
          this.mem.set("eof", !h || h.eof ? 1 : 0);
          break;
        }
        case "Spawn": {
          for (const p of a.map(text)) {
            const w = new Rt(this.host, null);
            w.runFile(p);
            this.workers.push({ rt: w, results: [], stopped: false });
            this.nums.set("threads", this.workers.length - 1);
          }
          break;
        }
        case "Kill": {
          this.need(1, W);
          const k = this.worker(this.x(0));
          if (k < 0) this.mem.set("err", "Invalid thread index");
          else this.workers[k] = null;
          break;
        }
        case "Send": {
          this.need(2, W);
          const m = this.t(0);
          const k = this.worker(this.x(1));
          if (k < 0) { this.mem.set("err", "Invalid thread index"); break; }
          const wk = this.workers[k];
          if (wk.stopped) { this.mem.set("err", "thread closed"); break; }
          if (m === "__stop__") { wk.stopped = true; break; }
          wk.rt.runSrc(m);
          const o = wk.rt.takeMem("out");
          if (o !== null) wk.results.push(o);
          break;
        }
        case "Recv": {
          this.need(1, W);
          const k = this.worker(this.x(0));
          this.args = [];
          if (k < 0) { this.mem.set("err", "Invalid thread index"); return false; }
          const wk = this.workers[k];
          if (wk.results.length) this.runSrc(wk.results.shift());
          else if (wk.stopped) this.mem.set("err", "thread closed");
          else this.fail(`>t: worker ${k} has no pending result; the program would wait forever`);
          return false;
        }
        case "PrngN": case "PrngS": {
          if (a.length === 1) this.rng = new Rng(seedU(this.x(0)), 0n);
          else if (a.length > 1) this.rng = new Rng(seedU(this.x(0)), seedU(this.x(1)));
          const d = this.dest(2, "rn");
          if (c === "PrngN") this.nums.set(d, this.rng.f64());
          else {
            const AZ = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
            let s = "";
            for (let i = 0; i < 32; i++) s += AZ[Number(this.rng.next() % 52n)];
            this.mem.set(d, s);
          }
          break;
        }
        case "Replace": this.need(2, W); this.repl.set(this.t(1), a[0]); break;
        case "Comment": break;
        case "Ret": return true;
        case "Exit": throw new SExit(a.length ? Math.max(-2147483648, Math.min(2147483647, toI64(num(a[0])))) : 0);
      }
      this.args = [];
      return false;
    }

    writeAt(h, off, data) {
      const vfs = this.host.vfs;
      const old = vfs.get(h.path) || new Uint8Array(0);
      const at = off === null ? old.length : off;
      const b = new Uint8Array(Math.max(old.length, at + data.length));
      b.set(old);
      b.set(data, at);
      vfs.set(h.path, b);
      h.pos = at + data.length;
      this.host.changed();
    }

    state() {
      const obj = (m) => [...m].map(([k, v]) => [k, text(v)]);
      return {
        stack: this.args.map(text),
        ow: this.ow,
        mem: obj(this.mem),
        nums: obj(this.nums),
        lists: [...this.lists].map(([k, l]) => [k, l.map(text)]),
        scripts: [...this.scripts.keys()],
        repl: obj(this.repl),
      };
    }
  }

  // runs a program; host: { vfs: Map<path, Uint8Array>, out, err, stdin: string[], tick, changed }
  function run(host, path, argv) {
    host.stdinPos = 0;
    const rt = new Rt(host, argv);
    let code = 0, error = null;
    try {
      rt.runFile(path);
    } catch (e) {
      if (e instanceof SExit) code = e.code;
      else if (e instanceof SErr) { error = e.message; code = 1; }
      else if (e instanceof RangeError) { error = "recursion too deep: " + e.message; code = 1; }
      else throw e;
    }
    return { code, error, state: rt.state() };
  }

  return { parse, classify, cmdOf, CMDS, SUFFIX, run, Rt, fmtNum, canon, SErr, isWs };
})();
if (typeof module !== "undefined") module.exports = SRT;
