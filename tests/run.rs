use std::path::{Path, PathBuf};
use std::process::Command;

fn cases() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().map_or(false, |e| e == "s") && p.with_extension("out").exists())
        .collect();
    v.sort();
    v
}

fn s() -> &'static str {
    env!("CARGO_BIN_EXE_s")
}

fn check(case: &Path, got: &[u8], how: &str) {
    let want = std::fs::read(case.with_extension("out")).unwrap();
    assert!(
        got == want.as_slice(),
        "{} ({}):\n--- want\n{}\n--- got\n{}",
        case.display(),
        how,
        String::from_utf8_lossy(&want),
        String::from_utf8_lossy(got)
    );
}

#[test]
fn interpreted() {
    for c in cases() {
        let o = Command::new(s()).arg("run").arg(c.file_name().unwrap()).current_dir(c.parent().unwrap()).output().unwrap();
        assert!(o.status.success(), "{}: {}", c.display(), String::from_utf8_lossy(&o.stderr));
        check(&c, &o.stdout, "interpreted");
    }
}

#[test]
fn compiled() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("s-compiled");
    std::fs::create_dir_all(&out).unwrap();
    for c in cases() {
        let bin = out.join(c.file_stem().unwrap());
        let b = Command::new(s()).arg("build").arg(&c).arg("-o").arg(&bin).output().unwrap();
        assert!(b.status.success(), "{}: {}", c.display(), String::from_utf8_lossy(&b.stderr));
        let o = Command::new(&bin).current_dir(c.parent().unwrap()).output().unwrap();
        assert!(o.status.success(), "{}: {}", c.display(), String::from_utf8_lossy(&o.stderr));
        check(&c, &o.stdout, "compiled");
    }
}

#[test]
fn compiled_with_embedded_files() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("s-embedded");
    std::fs::create_dir_all(&out).unwrap();
    let bin = out.join("threads");
    let b = Command::new(s())
        .args(["build", "threads.s", "-e", "threads_worker.s", "-e", "par_a.s", "-e", "par_b.s", "-o"])
        .arg(&bin)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let o = Command::new(&bin).current_dir(&out).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    check(&dir.join("threads.s"), &o.stdout, "embedded, run outside the source dir");
}
