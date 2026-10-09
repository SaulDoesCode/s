use s::compile::compile;
use s::rt::{s_main, Entry, EMPTY};
use std::path::Path;
use std::process::{exit, Command};

const USAGE: &str = "usage:
  s [file.s] [args...]                 interpret (default ./main.s)
  s --src file.s [args...]             interpret
  s run file.s [args...]               interpret
  s rs file.s [-o out.rs] [-e file]... emit a Rust program
  s build file.s [-o bin] [-e file]... [--keep] emit and compile with rustc -O";

fn die(m: &str) -> ! {
    eprintln!("s: {}", m);
    exit(2)
}

fn read(p: &str) -> String {
    match std::fs::read(p) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => die(&format!("{}: {}", p, e)),
    }
}

struct Opts {
    file: String,
    out: Option<String>,
    embeds: Vec<(String, String)>,
    keep: bool,
}

fn opts(a: &[String]) -> Opts {
    let mut o = Opts { file: String::new(), out: None, embeds: Vec::new(), keep: false };
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "-o" => {
                i += 1;
                o.out = Some(a.get(i).cloned().unwrap_or_else(|| die("-o needs a path")));
            }
            "-e" => {
                i += 1;
                let p = a.get(i).cloned().unwrap_or_else(|| die("-e needs a path"));
                let t = read(&p);
                o.embeds.push((p, t));
            }
            "--keep" => o.keep = true,
            f if o.file.is_empty() => o.file = f.to_string(),
            f => die(&format!("unexpected argument '{}'\n{}", f, USAGE)),
        }
        i += 1;
    }
    if o.file.is_empty() {
        die(USAGE)
    }
    o
}

fn emit(o: &Opts) -> String {
    let src = read(&o.file);
    match compile((&o.file, &src), &o.embeds) {
        Ok(rs) => rs,
        Err(e) => die(&e),
    }
}

fn stem(p: &str) -> String {
    Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "out".into())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        None => s_main(&EMPTY, Entry::File("./main.s".into()), Vec::new()),
        Some("-h") | Some("--help") => {
            println!("{}", USAGE);
        }
        Some("run") | Some("--src") => {
            let f = a.get(1).cloned().unwrap_or_else(|| die(USAGE));
            s_main(&EMPTY, Entry::File(f), a[2..].to_vec())
        }
        Some("rs") => {
            let o = opts(&a[1..]);
            let rs = emit(&o);
            match &o.out {
                Some(p) => std::fs::write(p, rs).unwrap_or_else(|e| die(&format!("{}: {}", p, e))),
                None => print!("{}", rs),
            }
        }
        Some("build") => {
            let o = opts(&a[1..]);
            let rs = emit(&o);
            let bin = o.out.clone().unwrap_or_else(|| stem(&o.file));
            let rsp = format!("{}.rs", bin);
            std::fs::write(&rsp, rs).unwrap_or_else(|e| die(&format!("{}: {}", rsp, e)));
            let st = Command::new("rustc")
                .args(["-O", "--edition", "2021", "--crate-name", "s_program", "-o", &bin, &rsp])
                .status()
                .unwrap_or_else(|e| die(&format!("rustc: {}", e)));
            if !o.keep {
                let _ = std::fs::remove_file(&rsp);
            }
            if !st.success() {
                die("rustc failed")
            }
        }
        Some(_) => s_main(&EMPTY, Entry::File(a[0].clone()), a[1..].to_vec()),
    }
}
