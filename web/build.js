// node web/build.js  ->  web/s-editor.html (runtime and examples inlined)
const fs = require("fs"), path = require("path");
const here = __dirname, cases = path.join(here, "../tests/cases"), ex = path.join(here, "examples");
const read = (d, f) => fs.readFileSync(path.join(d, f), "utf8");
const examples = [
  { id: "tour", name: "tour (main.s)", files: [["main.s", read(ex, "main.s")]] },
  { id: "gematria", name: "gematria", files: [["gematria.s", read(cases, "gematria.s")]] },
  { id: "lines", name: "stdin lines", files: [["lines.s", read(ex, "lines.s")]], stdin: "the quick brown fox\njumps\nover the lazy dog" },
  { id: "scripts", name: "scripts and calls", files: [["scripts.s", read(cases, "scripts.s")]] },
  { id: "tail", name: "tail calls", files: [["tail.s", read(cases, "tail.s")]] },
  { id: "cond", name: "conditions", files: [["cond.s", read(cases, "cond.s")]] },
  { id: "lists", name: "lists", files: [["lists.s", read(cases, "lists.s")]] },
  { id: "match", name: "?) tables", files: [["match.s", read(cases, "match.s")]] },
  { id: "basics", name: "literals", files: [["basics.s", read(cases, "basics.s")]] },
  { id: "files", name: "files and shell", files: [["files.s", read(cases, "files.s")]] },
  { id: "threads", name: "threads", files: ["threads.s", "threads_worker.s", "par_a.s", "par_b.s"].map((f) => [f, read(cases, f)]) },
];
let html = fs.readFileSync(path.join(here, "editor.template.html"), "utf8");
const json = JSON.stringify(examples).replace(/</g, "\\u003c");
html = html.split("/*SRT*/").join(fs.readFileSync(path.join(here, "srt.js"), "utf8").replace(/<\/script/gi, "<\\/script"));
html = html.split("/*EXAMPLES*/").join(json);
fs.writeFileSync(path.join(here, "s-editor.html"), html);
console.log("wrote s-editor.html", html.length, "bytes");
