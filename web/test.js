const fs = require("fs"), path = require("path");
const SRT = require("./srt.js");
const dir = path.join(__dirname, "../tests/cases");
let fails = 0;
for (const f of fs.readdirSync(dir).filter((f) => f.endsWith(".s") && fs.existsSync(path.join(dir, f.replace(/\.s$/, ".out")))).sort()) {
  const vfs = new Map();
  for (const g of fs.readdirSync(dir)) vfs.set(g, new Uint8Array(fs.readFileSync(path.join(dir, g))));
  let out = "", err = "";
  const host = { vfs, out: (s) => (out += s), err: (s) => (err += s), stdin: [], tick() {}, changed() {} };
  const r = SRT.run(host, f, []);
  const want = fs.readFileSync(path.join(dir, f.replace(/\.s$/, ".out")), "utf8");
  const ok = out === want && r.code === 0;
  if (!ok) { fails++; console.log("FAIL", f, r.error || "", "\n--- want\n" + want + "\n--- got\n" + out); }
  else console.log("ok  ", f);
}
// number formatting parity
const cases = [[1e21, "1000000000000000000000"], [1e-7, "0.0000001"], [-0, "0"], [0.1 + 0.2, "0.30000000000000004"], [1 / 3, "0.3333333333333333"], [123456789012345680000, "123456789012345680000"], [2.5e-10, "0.00000000025"]];
for (const [x, s] of cases) if (SRT.fmtNum(x) !== s) { fails++; console.log("fmt", x, SRT.fmtNum(x), s); }
process.exit(fails ? 1 : 0);
