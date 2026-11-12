//! Ścieżki CI: `paths:` workflow boxa (albo kontenera) wynikają z grafu zależności,
//! więc nie przepisujemy ich ręcznie. `shrimp-box ci --write` je ustawia, `check` pilnuje zgodności.

use crate::graph;
use crate::model::Repo;
use std::fs;

pub fn workflow_path(name: &str) -> String {
    format!(".github/workflows/{name}.yml")
}

/// Wzorce `paths:` dla boxa: on sam, jego zależności (przechodnio) i jego workflow.
pub fn box_paths(repo: &Repo, id: &str) -> Result<Vec<String>, String> {
    let mut v: Vec<String> = Vec::new();
    for b in graph::closure(repo, id)? {
        push(&mut v, repo.get(&b)?.path_glob());
    }
    if let Some(ci) = &repo.get(id)?.meta().ci {
        push(&mut v, workflow_path(ci));
    }
    Ok(v)
}

/// Wzorce `paths:` dla kontenera: wszystkie jego boxy z zależnościami, narzędzie shrimp-box, `ci_extra`, workflow.
pub fn container_paths(repo: &Repo, id: &str) -> Result<Vec<String>, String> {
    let mut v: Vec<String> = Vec::new();
    for m in graph::members(repo, id)? {
        for b in graph::closure(repo, &m.id)? {
            push(&mut v, repo.get(&b)?.path_glob());
        }
    }
    push(&mut v, "boxes/shrimp-box/**".into());
    push(&mut v, format!("containers/{id}.toml"));
    for inc in included(repo, id)? {
        push(&mut v, format!("containers/{inc}.toml"));
    }
    let c = repo.container(id)?;
    for e in &c.meta().ci_extra {
        push(&mut v, e.clone());
    }
    if let Some(ci) = &c.meta().ci {
        push(&mut v, workflow_path(ci));
    }
    Ok(v)
}

fn included(repo: &Repo, id: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for inc in &repo.container(id)?.meta().include {
        out.push(inc.clone());
        out.extend(included(repo, inc)?);
    }
    Ok(out)
}

fn push(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// Linia `paths:` w stylu workflow repo: `paths: ["a/**", "b"]`.
pub fn render(paths: &[String]) -> String {
    let items: Vec<String> = paths.iter().map(|p| format!("\"{p}\"")).collect();
    format!("paths: [{}]", items.join(", "))
}

/// Wszystkie pary (workflow, oczekiwane paths) z boxów i kontenerów.
pub fn expected(repo: &Repo) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for b in repo.boxes.values() {
        if let Some(ci) = &b.meta().ci {
            out.push((ci.clone(), box_paths(repo, b.id())?));
        }
    }
    for c in repo.containers.values() {
        if let Some(ci) = &c.meta().ci {
            out.push((ci.clone(), container_paths(repo, &c.meta().id)?));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for (ci, _) in &out {
        if !seen.insert(ci.clone()) {
            return Err(format!("workflow {ci} jest przypisany do dwóch boxów albo kontenerów"));
        }
    }
    Ok(out)
}

/// Podmienia każdą linię `paths: [...]` w treści workflow. Zwraca (nowa treść, liczba linii).
pub fn rewrite(text: &str, paths: &[String]) -> (String, usize) {
    let mut n = 0;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let indent = &body[..body.len() - body.trim_start().len()];
        if body.trim_start().starts_with("paths: [") && body.trim_end().ends_with(']') {
            out.push_str(indent);
            out.push_str(&render(paths));
            out.push_str(&line[body.len()..]);
            n += 1;
        } else {
            out.push_str(line);
        }
    }
    (out, n)
}

/// Sprawdza (albo z `write` ustawia) `paths:` we wszystkich workflow (workflow bez `paths:` działa zawsze). Zwraca opisy niezgodności.
pub fn sync(repo: &Repo, write: bool) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    for (ci, paths) in expected(repo)? {
        let rel = workflow_path(&ci);
        let p = repo.root.join(&rel);
        let text = fs::read_to_string(&p).map_err(|e| format!("{rel}: {e}"))?;
        let (new, n) = rewrite(&text, &paths);
        // workflow bez `paths:` działa przy każdej zmianie: nic do ustawiania
        if n > 0 && new != text {
            if write {
                fs::write(&p, new).map_err(|e| format!("{rel}: {e}"))?;
                println!("zapisano {rel}");
            } else {
                problems.push(format!("{rel}: paths nieaktualne, oczekiwane: {}", render(&paths)));
            }
        }
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_keeps_indent_and_other_lines() {
        let text = "on:\n  push:\n    paths: [\"old/**\"]\n  pull_request:\n    paths: [\"old/**\"]\n  workflow_dispatch:\n";
        let (out, n) = rewrite(text, &["boxes/a/**".into(), ".github/workflows/a.yml".into()]);
        assert_eq!(n, 2);
        assert_eq!(
            out,
            "on:\n  push:\n    paths: [\"boxes/a/**\", \".github/workflows/a.yml\"]\n  pull_request:\n    paths: [\"boxes/a/**\", \".github/workflows/a.yml\"]\n  workflow_dispatch:\n"
        );
    }
}
