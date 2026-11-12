//! `shrimp-box check`: zasady boxów i kontenerów. Każde naruszenie to błąd (CI pada).

use crate::model::{Kind, Repo};
use crate::{ci, graph, kiwi};
use std::fs;

/// Co może leżeć w korzeniu repo poza boxami (narzędzia repo, CI, opisy).
const TOP_LEVEL: &[&str] = &[
    "boxes", "containers", "tools", ".github", ".git", ".gitignore", "README.md", "OPEN-SOURCE.md", "out", "test-results",
];

pub fn check(repo: &Repo) -> Vec<String> {
    let mut errs: Vec<String> = repo.load_errors.clone();
    let root = &repo.root;

    for b in repo.boxes.values() {
        let m = b.meta();
        let at = |what: String| format!("box {}: {what}", b.id());
        for d in m.depends.iter().chain(&m.build_depends) {
            if d == b.id() {
                errs.push(at("zależy od samego siebie".into()));
            } else if !repo.boxes.contains_key(d) {
                errs.push(at(format!("nieznana zależność „{d}”")));
            }
        }
        if !["linux", "windows", "web"].contains(&m.platform.as_str()) {
            errs.push(at(format!("nieznana platforma „{}”", m.platform)));
        }
        let mut files: Vec<(&str, String)> = Vec::new();
        if let Some(i) = &b.file.build.install {
            files.push(("build.install", i.clone()));
        }
        if let Some(c) = &b.file.image.configure {
            files.push(("image.configure", c.clone()));
        }
        if let Some(t) = &b.file.image.kiwi_template {
            files.push(("image.kiwi_template", t.clone()));
        }
        for f in &b.file.image.kiwi_files {
            files.push(("image.kiwi_files", f.clone()));
        }
        for (what, f) in files {
            if !root.join(&b.dir).join(&f).is_file() {
                errs.push(at(format!("{what}: brak pliku {}/{f}", b.dir.display())));
            }
        }
        if let Some(i) = &b.file.build.install {
            if !is_executable(&root.join(&b.dir).join(i)) {
                errs.push(at(format!("{i} nie jest wykonywalny")));
            }
        }
        if !b.file.build.outputs.is_empty() && b.file.build.chroot.is_none() {
            errs.push(at("build.outputs bez build.chroot".into()));
        }
        if matches!(m.kind, Kind::Lib | Kind::Data)
            && (b.file.build.install.is_some() || b.configure(root).is_some() || b.root_overlay(root).is_some())
        {
            errs.push(at("lib/data niczego nie instaluje (bez install, configure.sh i root/)".into()));
        }
        if let Some(ciname) = &m.ci {
            if !root.join(ci::workflow_path(ciname)).is_file() {
                errs.push(at(format!("brak workflow {}", ci::workflow_path(ciname))));
            }
        }
    }
    errs.extend(graph::cycles(repo).into_iter().map(|c| format!("cykl zależności: {c}")));

    for c in repo.containers.values() {
        let id = &c.meta().id;
        match graph::order(repo, id) {
            Err(e) => errs.push(e),
            Ok(members) => {
                for m in &members {
                    let b = &repo.boxes[&m.id];
                    if b.meta().platform != c.meta().platform {
                        errs.push(format!(
                            "kontener {id} ({}): box {} jest na platformę {}",
                            c.meta().platform,
                            m.id,
                            b.meta().platform
                        ));
                    }
                    if matches!(b.meta().kind, Kind::Lib | Kind::Data) {
                        errs.push(format!("kontener {id}: {} to lib/data, wchodzi tylko jako zależność budowania", m.id));
                    }
                }
                let templates = members.iter().filter(|m| repo.boxes[&m.id].file.image.kiwi_template.is_some()).count();
                if templates > 1 {
                    errs.push(format!("kontener {id}: więcej niż jeden szablon config.kiwi"));
                }
                match kiwi::overlay_conflicts(repo, id) {
                    Ok(v) => errs.extend(v),
                    Err(e) => errs.push(e),
                }
            }
        }
    }

    errs.extend(undeclared_references(repo));

    match ci::sync(repo, false) {
        Ok(v) => errs.extend(v),
        Err(e) => errs.push(e),
    }

    // Box, którego nic nie używa (ani kontener, ani zależność, ani CI), to martwy kod.
    for b in repo.boxes.values() {
        let used = b.meta().ci.is_some()
            || repo.boxes.values().any(|o| o.meta().depends.iter().chain(&o.meta().build_depends).any(|d| d == b.id()))
            || repo.containers.values().any(|c| c.meta().boxes.iter().any(|e| e.id() == b.id()));
        if !used {
            errs.push(format!("box {}: nikt go nie używa (kontener, zależność albo ci)", b.id()));
        }
    }

    if let Ok(rd) = fs::read_dir(root) {
        let mut extra: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !TOP_LEVEL.contains(&n.as_str()))
            .collect();
        extra.sort();
        for n in extra {
            errs.push(format!("{n}: poza boxes/ (kod należy do boxa; narzędzia repo do tools/)"));
        }
    }
    errs
}

/// Rozszerzenia plików z kodem, w których szukamy odwołań `../<box>/` do innych boxów.
const CODE: &[&str] = &["rs", "ts", "js", "mjs", "sh", "py", "toml", "c", "h"];
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "root", "i18n", "data", ".git"];

/// Zasada 2: box sięga do innego boxa (`../<id>/` w kodzie) tylko, gdy ma go w zależnościach.
fn undeclared_references(repo: &Repo) -> Vec<String> {
    let mut errs = Vec::new();
    for b in repo.boxes.values() {
        let allowed = graph::closure(repo, b.id()).unwrap_or_default();
        let mut files = Vec::new();
        collect(&repo.root.join(&b.dir), &mut files);
        for f in files {
            let Ok(text) = fs::read_to_string(&f) else { continue };
            for other in repo.boxes.keys().filter(|o| *o != b.id()) {
                let hit = ["/", "\"", "'"].iter().any(|end| text.contains(&format!("../{other}{end}")));
                if hit && !allowed.contains(other) {
                    let rel = f.strip_prefix(&repo.root).unwrap_or(&f).display().to_string();
                    errs.push(format!("box {}: {rel} sięga do boxa {other}, którego nie ma w depends/build_depends", b.id()));
                }
            }
        }
    }
    errs
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        if p.is_dir() {
            if !SKIP_DIRS.iter().any(|s| name == *s) {
                collect(&p, out);
            }
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| CODE.contains(&x)) {
            out.push(p);
        }
    }
}

fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}
