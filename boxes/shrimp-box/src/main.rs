//! shrimp-box: boxy (kod + box.toml) i kontenery (worki boxów) repozytorium Shrimp.
//! Opis architektury: boxes/README.md.

mod check;
mod ci;
mod fsx;
mod graph;
mod kiwi;
mod make;
mod model;

use model::Repo;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
shrimp-box [--root KATALOG] POLECENIE

  list                         boxy i kontenery
  check                        zasady: zależności, cykle, kontenery, pliki, ścieżki CI
  order KONTENER               kolejność boxów (z argumentami)
  deps BOX                     box i jego zależności (przechodnio)
  paths BOX|KONTENER           wzorce paths: dla CI
  ci [--write]                 sprawdza (albo ustawia) paths: we wszystkich workflow
  graph                        graf zależności (Graphviz dot)
  kiwi KONTENER WYJŚCIE [--repo URL]
                               opis obrazu KIWI: config.kiwi, config.sh, root/, pliki obok
  build KONTENER DESTDIR [--repo URL] [--buildroot KATALOG] [--dry-run]
                               kroki chroot Leap i instalacja boxów do DESTDIR
  image KONTENER WYJŚCIE --repo URL [--buildroot KATALOG] [--dry-run]
                               kiwi + build do WYJŚCIE/root (całość przed kiwi-ng)
";

fn find_root(start: &Path) -> Option<PathBuf> {
    let mut p = start.to_path_buf();
    loop {
        if p.join("boxes").is_dir() && p.join("containers").is_dir() {
            return Some(p);
        }
        if !p.pop() {
            return None;
        }
    }
}

struct Args {
    pos: Vec<String>,
    flags: Vec<String>,
    repo: Option<String>,
    buildroot: Option<String>,
    root: Option<String>,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args { pos: vec![], flags: vec![], repo: None, buildroot: None, root: None };
    while let Some(s) = it.next() {
        match s.as_str() {
            "--repo" => a.repo = Some(it.next().ok_or("--repo bez wartości")?),
            "--buildroot" => a.buildroot = Some(it.next().ok_or("--buildroot bez wartości")?),
            "--root" => a.root = Some(it.next().ok_or("--root bez wartości")?),
            f if f.starts_with("--") => a.flags.push(f.into()),
            _ => a.pos.push(s),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("shrimp-box: {e}");
            ExitCode::FAILURE
        }
    }
}

fn abs(p: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(p);
    if p.is_absolute() {
        Ok(p)
    } else {
        Ok(std::env::current_dir().map_err(|e| e.to_string())?.join(p))
    }
}

fn run() -> Result<ExitCode, String> {
    let a = parse(std::env::args().skip(1))?;
    let flag = |f: &str| a.flags.iter().any(|x| x == f);
    if a.pos.is_empty() || flag("--help") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let root = match &a.root {
        Some(r) => abs(r)?,
        None => find_root(&std::env::current_dir().map_err(|e| e.to_string())?)
            .ok_or("nie znaleziono korzenia repo (katalogi boxes/ i containers/)")?,
    };
    let repo = Repo::load(&root);
    let arg = |i: usize, what: &str| a.pos.get(i).cloned().ok_or(format!("brak argumentu: {what}\n\n{USAGE}"));
    let buildroot = a.buildroot.clone().unwrap_or_else(|| "/var/tmp/shrimp-buildroot".into());

    match a.pos[0].as_str() {
        "list" => {
            println!("BOXY");
            for b in repo.boxes.values() {
                let m = b.meta();
                println!("  {:<18} {:<8} {:<7} {}", m.id, format!("{:?}", m.kind).to_lowercase(), m.platform, m.summary);
            }
            println!("KONTENERY");
            for c in repo.containers.values() {
                let n = graph::members(&repo, &c.meta().id).map(|v| v.len()).unwrap_or(0);
                println!("  {:<18} {:>2} boxów  {}", c.meta().id, n, c.meta().summary);
            }
        }
        "check" => {
            let errs = check::check(&repo);
            for e in &errs {
                println!("BŁĄD: {e}");
            }
            if !errs.is_empty() {
                return Ok(ExitCode::FAILURE);
            }
            println!("ok: {} boxów, {} kontenerów", repo.boxes.len(), repo.containers.len());
        }
        "order" => {
            for m in graph::order(&repo, &arg(1, "KONTENER")?)? {
                println!("{}{}", m.id, if m.args.is_empty() { String::new() } else { format!(" {}", m.args.join(" ")) });
            }
        }
        "deps" => {
            for d in graph::closure(&repo, &arg(1, "BOX")?)? {
                println!("{d}");
            }
        }
        "paths" => {
            let id = arg(1, "BOX|KONTENER")?;
            let v = if repo.containers.contains_key(&id) { ci::container_paths(&repo, &id)? } else { ci::box_paths(&repo, &id)? };
            println!("{}", ci::render(&v));
        }
        "ci" => {
            let problems = ci::sync(&repo, flag("--write"))?;
            for p in &problems {
                println!("BŁĄD: {p}");
            }
            if !problems.is_empty() {
                return Ok(ExitCode::FAILURE);
            }
        }
        "graph" => {
            println!("digraph shrimp {{\n  rankdir=LR;\n  node [shape=box];");
            for b in repo.boxes.values() {
                for d in &b.meta().depends {
                    println!("  \"{}\" -> \"{d}\";", b.id());
                }
                for d in &b.meta().build_depends {
                    println!("  \"{}\" -> \"{d}\" [style=dashed];", b.id());
                }
            }
            println!("}}");
        }
        "kiwi" => {
            let out = abs(&arg(2, "WYJŚCIE")?)?;
            let n = kiwi::describe(&repo, &arg(1, "KONTENER")?, &out, a.repo.as_deref().unwrap_or(""))?;
            println!("opis obrazu w {} ({n} plików root/)", out.display());
        }
        "build" | "image" => {
            let container = arg(1, "KONTENER")?;
            let out = abs(&arg(2, "KATALOG")?)?;
            let image = a.pos[0] == "image";
            let dest = if image {
                let url = a.repo.as_deref().ok_or("image potrzebuje --repo URL")?;
                let n = kiwi::describe(&repo, &container, &out, url)?;
                println!("opis obrazu w {} ({n} plików root/)", out.display());
                out.join("root")
            } else {
                out
            };
            std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            let opts = make::Opts { repo_url: a.repo.clone(), buildroot: abs(&buildroot)?, dry_run: flag("--dry-run") };
            let installed = make::build(&repo, &container, &dest, &opts)?;
            if image {
                // manifesty z plikami install.sh (post-install Setup kopiuje je do zainstalowanego systemu)
                kiwi::write_manifests(&repo, &container, &dest, &installed)?;
            }
        }
        other => return Err(format!("nieznane polecenie „{other}”\n\n{USAGE}")),
    }
    Ok(ExitCode::SUCCESS)
}
