//! Graf zależności: kolejność boxów w kontenerze, domknięcie zależności, cykle.

use crate::model::Repo;
use std::collections::{BTreeSet, HashMap};

/// Box w kontenerze z argumentami z pozycji kontenera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub args: Vec<String>,
}

/// Lista boxów kontenera: najpierw boxy kontenerów z `include` (po kolei), potem własne.
/// Box powtórzony w dwóch miejscach to błąd (kontener jest workiem, nie listą z duplikatami).
pub fn members(repo: &Repo, container: &str) -> Result<Vec<Member>, String> {
    let mut out = Vec::new();
    let mut stack = Vec::new();
    expand(repo, container, &mut stack, &mut out)?;
    let mut seen = BTreeSet::new();
    for m in &out {
        if !seen.insert(m.id.clone()) {
            return Err(format!("kontener {container}: box „{}” jest w nim dwa razy", m.id));
        }
    }
    Ok(out)
}

fn expand(repo: &Repo, id: &str, stack: &mut Vec<String>, out: &mut Vec<Member>) -> Result<(), String> {
    if stack.iter().any(|s| s == id) {
        stack.push(id.into());
        return Err(format!("cykl kontenerów: {}", stack.join(" → ")));
    }
    let c = repo.container(id)?;
    stack.push(id.into());
    for inc in &c.meta().include {
        expand(repo, inc, stack, out)?;
    }
    for e in &c.meta().boxes {
        repo.get(e.id()).map_err(|err| format!("kontener {id}: {err}"))?;
        out.push(Member { id: e.id().into(), args: e.args().to_vec() });
    }
    stack.pop();
    Ok(())
}

/// Kolejność budowy i konfiguracji: kolejność z kontenera, ale każdy box po swoich `depends`
/// (stabilne sortowanie topologiczne). Zależność spoza kontenera to błąd.
pub fn order(repo: &Repo, container: &str) -> Result<Vec<Member>, String> {
    let list = members(repo, container)?;
    let index: HashMap<&str, usize> = list.iter().enumerate().map(|(i, m)| (m.id.as_str(), i)).collect();
    let mut missing = Vec::new();
    let mut indeg = vec![0usize; list.len()];
    let mut after: Vec<Vec<usize>> = vec![vec![]; list.len()];
    for (i, m) in list.iter().enumerate() {
        for d in &repo.get(&m.id)?.meta().depends {
            match index.get(d.as_str()) {
                Some(&j) => {
                    indeg[i] += 1;
                    after[j].push(i);
                }
                None => missing.push(format!("{} potrzebuje „{d}”", m.id)),
            }
        }
    }
    if !missing.is_empty() {
        return Err(format!("kontener {container}: brak zależności: {}", missing.join(", ")));
    }
    let mut ready: BTreeSet<usize> = (0..list.len()).filter(|&i| indeg[i] == 0).collect();
    let mut out = Vec::with_capacity(list.len());
    while let Some(&i) = ready.iter().next() {
        ready.remove(&i);
        out.push(list[i].clone());
        for &k in &after[i] {
            indeg[k] -= 1;
            if indeg[k] == 0 {
                ready.insert(k);
            }
        }
    }
    if out.len() != list.len() {
        let rest: Vec<_> = list.iter().filter(|m| !out.contains(m)).map(|m| m.id.as_str()).collect();
        return Err(format!("kontener {container}: cykl zależności między {}", rest.join(", ")));
    }
    Ok(out)
}

/// Box i wszystkie jego zależności (depends + build_depends), przechodnio; w kolejności odkrycia.
pub fn closure(repo: &Repo, id: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut todo = vec![id.to_string()];
    while let Some(b) = todo.pop() {
        if out.contains(&b) {
            continue;
        }
        let bx = repo.get(&b)?;
        out.push(b);
        for d in bx.meta().depends.iter().chain(&bx.meta().build_depends).rev() {
            todo.push(d.clone());
        }
    }
    Ok(out)
}

/// Wszystkie cykle w grafie depends ∪ build_depends (każdy raz, jako ścieżka).
pub fn cycles(repo: &Repo) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum St {
        New,
        Open,
        Done,
    }
    let ids: Vec<&String> = repo.boxes.keys().collect();
    let mut st: HashMap<&str, St> = ids.iter().map(|i| (i.as_str(), St::New)).collect();
    let mut found = Vec::new();
    fn visit<'a>(repo: &'a Repo, id: &'a str, st: &mut HashMap<&'a str, St>, path: &mut Vec<&'a str>, found: &mut Vec<String>) {
        st.insert(id, St::Open);
        path.push(id);
        if let Some(b) = repo.boxes.get(id) {
            for d in b.meta().depends.iter().chain(&b.meta().build_depends) {
                match st.get(d.as_str()).copied() {
                    Some(St::New) => visit(repo, d, st, path, found),
                    Some(St::Open) => {
                        let start = path.iter().position(|p| *p == d).unwrap_or(0);
                        let mut c: Vec<&str> = path[start..].to_vec();
                        c.push(d);
                        found.push(c.join(" → "));
                    }
                    _ => {}
                }
            }
        }
        path.pop();
        st.insert(id, St::Done);
    }
    for id in ids {
        if st[id.as_str()] == St::New {
            visit(repo, id, &mut st, &mut Vec::new(), &mut found);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::path::PathBuf;

    fn bx(id: &str, depends: &[&str], build: &[&str]) -> (String, Boxx) {
        let text = format!(
            "[box]\nid = \"{id}\"\nversion = \"0.1.0\"\nkind = \"app\"\ndepends = {:?}\nbuild_depends = {:?}\n",
            depends, build
        );
        let file: BoxFile = toml::from_str(&text).unwrap();
        (id.into(), Boxx { file, dir: PathBuf::from("boxes").join(id) })
    }

    fn ct(id: &str, text: &str) -> (String, Container) {
        let file: ContainerFile = toml::from_str(text).unwrap();
        (id.into(), Container { file })
    }

    fn repo() -> Repo {
        let mut r = Repo::default();
        for (k, v) in [
            bx("lib", &[], &[]),
            bx("fonts", &[], &[]),
            bx("grub", &["fonts"], &[]),
            bx("app", &[], &["lib"]),
            bx("setup", &["app"], &["lib"]),
        ] {
            r.boxes.insert(k, v);
        }
        for (k, v) in [
            ct("desktop", "[container]\nid = \"desktop\"\nboxes = [\"grub\", \"fonts\", \"app\"]\n"),
            ct("live", "[container]\nid = \"live\"\ninclude = [\"desktop\"]\nboxes = [{ id = \"setup\", args = [\"--live\"] }]\n"),
            ct("broken", "[container]\nid = \"broken\"\nboxes = [\"setup\"]\n"),
            ct("loop", "[container]\nid = \"loop\"\ninclude = [\"loop\"]\n"),
        ] {
            r.containers.insert(k, v);
        }
        r
    }

    #[test]
    fn order_puts_dependencies_first_and_keeps_container_order() {
        let ids: Vec<String> = order(&repo(), "desktop").unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["fonts", "grub", "app"]);
    }

    #[test]
    fn include_and_args() {
        let o = order(&repo(), "live").unwrap();
        assert_eq!(o.last().unwrap(), &Member { id: "setup".into(), args: vec!["--live".into()] });
        assert_eq!(o.len(), 4);
    }

    #[test]
    fn missing_runtime_dependency_is_an_error() {
        let e = order(&repo(), "broken").unwrap_err();
        assert!(e.contains("setup potrzebuje „app”"), "{e}");
    }

    #[test]
    fn container_cycle_is_an_error() {
        assert!(members(&repo(), "loop").unwrap_err().contains("cykl kontenerów"));
    }

    #[test]
    fn closure_follows_build_depends() {
        assert_eq!(closure(&repo(), "setup").unwrap(), ["setup", "app", "lib"]);
    }

    #[test]
    fn cycles_are_found() {
        let mut r = repo();
        assert!(cycles(&r).is_empty());
        let (k, v) = bx("lib", &["setup"], &[]);
        r.boxes.insert(k, v);
        let c = cycles(&r);
        assert!(!c.is_empty());
        assert!(c.iter().all(|x| x.contains("lib") && x.contains("setup")), "{c:?}");
    }
}
