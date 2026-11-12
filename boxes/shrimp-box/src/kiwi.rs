//! Opis obrazu KIWI z kontenera: config.kiwi (szablon + pakiety boxów), config.sh (fragmenty boxów
//! w kolejności kontenera), root/ (nakładki `root/` boxów, bez konfliktów) i pliki obok config.kiwi.

use crate::fsx;
use crate::graph::{self, Member};
use crate::model::Repo;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const PACKAGES_MARKER: &str = "@SHRIMP_PACKAGES@";

/// Pakiety obrazu jako linie XML, pogrupowane po boxach; pakiet z wcześniejszego boxa się nie powtarza.
pub fn packages_xml(repo: &Repo, members: &[Member]) -> Result<String, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = String::new();
    for m in members {
        let b = repo.get(&m.id)?;
        let mut lines = Vec::new();
        for c in &b.file.packages.collections {
            if seen.insert(format!("@{c}")) {
                lines.push(format!("        <namedCollection name=\"{c}\"/>"));
            }
        }
        for p in &b.file.packages.image {
            if seen.insert(p.clone()) {
                lines.push(format!("        <package name=\"{p}\"/>"));
            }
        }
        if !lines.is_empty() {
            out.push_str(&format!("        <!-- box {}: {} -->\n", b.id(), xml_comment(&b.meta().summary)));
            for l in lines {
                out.push_str(&l);
                out.push('\n');
            }
        }
    }
    Ok(out.trim_end_matches('\n').to_string())
}

fn xml_comment(s: &str) -> String {
    s.replace("--", "–")
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// config.sh: nagłówek, fragmenty `configure.sh` boxów (każdy z BOX_ARGS z kontenera), `exit 0`.
pub fn config_sh(repo: &Repo, container: &str, members: &[Member], repo_url: &str) -> Result<String, String> {
    let mut s = format!(
        "#!/bin/bash\n\
         # Shrimp image configuration (runs chrooted inside the image root, after packages and root/).\n\
         # Wygenerowane przez shrimp-box z kontenera {container}: nie edytuj, fragmenty są w boxes/<box>/configure.sh.\n\
         set -euxo pipefail\n\n\
         test -f /.kconfig && . /.kconfig\n\
         test -f /.profile && . /.profile\n\n\
         die() {{ echo \"config.sh: $*\" >&2; exit 1; }}\n\
         export SHRIMP_CONTAINER={}\n\
         SHRIMP_REPO={}\n",
        sh_quote(container),
        sh_quote(repo_url.trim_end_matches('/'))
    );
    for m in members {
        let b = repo.get(&m.id)?;
        let Some(rel) = b.configure(&repo.root) else { continue };
        let text = fs::read_to_string(repo.root.join(&rel)).map_err(|e| format!("{}: {e}", rel.display()))?;
        let args: Vec<String> = m.args.iter().map(|a| a.to_string()).collect();
        s.push_str(&format!(
            "\n#--------------------------------------\n# box {} {} ({})\n#--------------------------------------\nBOX_ARGS={}\n",
            b.id(),
            b.meta().version,
            rel.display(),
            sh_quote(&args.join(" "))
        ));
        s.push_str(text.trim_end_matches('\n'));
        s.push('\n');
    }
    // usługi z `[image] enable` / `user_enable`: na końcu, gdy wszystkie pliki są już w obrazie
    for m in members {
        let img = &repo.get(&m.id)?.file.image;
        if !img.enable.is_empty() {
            s.push_str(&format!("systemctl enable {}  # box {}\n", img.enable.join(" "), m.id));
        }
        if !img.user_enable.is_empty() {
            s.push_str(&format!("systemctl --global enable {}  # box {}\n", img.user_enable.join(" "), m.id));
        }
    }
    s.push_str("\nexit 0\n");
    Ok(s)
}

/// Składa opis obrazu w `out` (katalog jest czyszczony). Zwraca liczbę plików nakładki root/.
pub fn describe(repo: &Repo, container: &str, out: &Path, repo_url: &str) -> Result<usize, String> {
    let members = graph::order(repo, container)?;
    let templates: Vec<_> = members
        .iter()
        .filter_map(|m| {
            let b = repo.boxes.get(&m.id)?;
            b.file.image.kiwi_template.as_ref().map(|t| (b, t))
        })
        .collect();
    let (tb, tpl) = match templates.as_slice() {
        [one] => *one,
        [] => return Err(format!("kontener {container}: żaden box nie ma image.kiwi_template")),
        _ => return Err(format!("kontener {container}: więcej niż jeden szablon config.kiwi")),
    };
    if out.exists() {
        fs::remove_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    fs::create_dir_all(out.join("root")).map_err(|e| e.to_string())?;

    let tpl_path = repo.root.join(&tb.dir).join(tpl);
    let tpl_text = fs::read_to_string(&tpl_path).map_err(|e| format!("{}: {e}", tpl_path.display()))?;
    // znacznik dokładnie raz (np. w komentarzu XML wstawione pakiety psują config.kiwi)
    if tpl_text.matches(PACKAGES_MARKER).count() != 1 {
        return Err(format!("{}: {PACKAGES_MARKER} musi wystąpić dokładnie raz", tpl_path.display()));
    }
    let kiwi = tpl_text.replace(PACKAGES_MARKER, &packages_xml(repo, &members)?);
    fs::write(out.join("config.kiwi"), kiwi).map_err(|e| e.to_string())?;

    let sh = config_sh(repo, container, &members, repo_url)?;
    let sh_path = out.join("config.sh");
    fs::write(&sh_path, sh).map_err(|e| e.to_string())?;
    fsx::chmod(&sh_path, 0o755)?;

    for m in &members {
        let b = repo.get(&m.id)?;
        for f in &b.file.image.kiwi_files {
            let src = repo.root.join(&b.dir).join(f);
            let name = Path::new(f).file_name().ok_or("zła nazwa pliku")?;
            fsx::copy_file(&src, &out.join(name))?;
        }
    }

    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for m in &members {
        let b = repo.get(&m.id)?;
        if let Some(overlay) = b.root_overlay(&repo.root) {
            let src = repo.root.join(&overlay);
            for rel in fsx::files(&src)? {
                let key = rel.to_string_lossy().into_owned();
                if let Some(prev) = owner.insert(key.clone(), b.id().to_string()) {
                    return Err(format!("plik /{key} jest w root/ boxów {prev} i {}", b.id()));
                }
                fsx::copy_file(&src.join(&rel), &out.join("root").join(&rel))?;
            }
        }
    }
    write_manifests(repo, container, &out.join("root"), &Installed::new())?;
    Ok(owner.len())
}

/// Katalog w obrazie z opisem kontenerów, z których go złożono.
pub const MANIFEST_DIR: &str = "usr/share/shrimp/containers";

/// Kontener i wszystkie kontenery z jego `include` (przechodnio), każdy raz.
fn container_tree(repo: &Repo, id: &str, out: &mut Vec<String>) -> Result<(), String> {
    if out.iter().any(|c| c == id) {
        return Ok(());
    }
    out.push(id.to_string());
    for inc in &repo.container(id)?.meta().include {
        container_tree(repo, inc, out)?;
    }
    Ok(())
}

/// Pliki położone przez `install.sh` każdego boxa (ścieżki bezwzględne w obrazie), z `make::build`.
pub type Installed = BTreeMap<String, Vec<String>>;

/// Dla kontenera obrazu i każdego dołączonego (np. desktop w iso-live) zapisuje w obrazie
/// `<id>.packages` (pakiety RPM `[packages] image` jego boxów), `<id>.files` (pliki nakładek root/
/// i pliki z `install.sh`, ścieżki bezwzględne) i `<id>.units` (`system|user <usługa>` z `[image] enable`).
/// Instalator (post-install Setup) dokłada z nich do zainstalowanego systemu to, czego Agama nie
/// instaluje z repozytorium, żeby pulpit po instalacji był taki jak na nośniku.
pub fn write_manifests(repo: &Repo, container: &str, root: &Path, installed: &Installed) -> Result<(), String> {
    let mut ids = Vec::new();
    container_tree(repo, container, &mut ids)?;
    let dir = root.join(MANIFEST_DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for id in ids {
        let mut pkgs = std::collections::BTreeSet::new();
        let mut files = std::collections::BTreeSet::new();
        let mut units = Vec::new();
        for m in graph::members(repo, &id)? {
            let b = repo.get(&m.id)?;
            pkgs.extend(b.file.packages.image.iter().cloned());
            files.extend(installed.get(&m.id).into_iter().flatten().cloned());
            units.extend(b.file.image.enable.iter().map(|u| format!("system {u}\n")));
            units.extend(b.file.image.user_enable.iter().map(|u| format!("user {u}\n")));
            if let Some(overlay) = b.root_overlay(&repo.root) {
                for rel in fsx::files(&repo.root.join(&overlay))? {
                    files.insert(format!("/{}", rel.to_string_lossy()));
                }
            }
        }
        let join = |set: std::collections::BTreeSet<String>| set.into_iter().map(|l| l + "\n").collect::<String>();
        fs::write(dir.join(format!("{id}.packages")), join(pkgs)).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{id}.files")), join(files)).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{id}.units")), units.concat()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Pliki root/ w kontenerze, które dwa boxy chcą położyć w tym samym miejscu.
pub fn overlay_conflicts(repo: &Repo, container: &str) -> Result<Vec<String>, String> {
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::new();
    for m in graph::members(repo, container)? {
        let b = repo.get(&m.id)?;
        if let Some(overlay) = b.root_overlay(&repo.root) {
            for rel in fsx::files(&repo.root.join(&overlay))? {
                let key = rel.to_string_lossy().into_owned();
                if let Some(prev) = owner.insert(key.clone(), b.id().to_string()) {
                    out.push(format!("kontener {container}: /{key} jest w root/ boxów {prev} i {}", b.id()));
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make;
    use std::os::unix::fs::PermissionsExt;

    fn write(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    /// Mały repo: box `app` instaluje plik i ma usługi, box `theme` ma nakładkę root/.
    #[test]
    fn manifests_list_overlay_and_installed_files_and_units() {
        let t = std::env::temp_dir().join(format!("shrimp-box-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
        write(&t.join("containers/desktop.toml"), "[container]\nid = \"desktop\"\nboxes = [\"theme\", \"app\"]\n");
        write(&t.join("boxes/theme/box.toml"), "[box]\nid = \"theme\"\nversion = \"0.1.0\"\nkind = \"theme\"\n");
        write(&t.join("boxes/theme/root/usr/share/theme.txt"), "x");
        write(
            &t.join("boxes/app/box.toml"),
            "[box]\nid = \"app\"\nversion = \"0.1.0\"\nkind = \"app\"\n[build]\ninstall = \"install.sh\"\n\
             [image]\nenable = [\"app.timer\"]\nuser_enable = [\"app-user.service\"]\n",
        );
        let inst = t.join("boxes/app/install.sh");
        write(&inst, "#!/bin/sh\nmkdir -p \"$1/usr/bin\" && echo app > \"$1/usr/bin/app\"\n");
        fs::set_permissions(&inst, fs::Permissions::from_mode(0o755)).unwrap();

        let repo = Repo::load(&t);
        assert!(repo.load_errors.is_empty(), "{:?}", repo.load_errors);
        let root = t.join("root");
        fs::create_dir_all(&root).unwrap();
        let opts = make::Opts { repo_url: None, buildroot: t.join("br"), dry_run: false };
        let installed = make::build(&repo, "desktop", &root, &opts).unwrap();
        assert_eq!(installed["app"], ["/usr/bin/app"]);
        write_manifests(&repo, "desktop", &root, &installed).unwrap();
        let m = root.join(MANIFEST_DIR);
        assert_eq!(fs::read_to_string(m.join("desktop.files")).unwrap(), "/usr/bin/app\n/usr/share/theme.txt\n");
        assert_eq!(fs::read_to_string(m.join("desktop.units")).unwrap(), "system app.timer\nuser app-user.service\n");

        let members = graph::order(&repo, "desktop").unwrap();
        let sh = config_sh(&repo, "desktop", &members, "").unwrap();
        assert!(sh.contains("systemctl enable app.timer"), "{sh}");
        assert!(sh.contains("systemctl --global enable app-user.service"), "{sh}");
        let _ = fs::remove_dir_all(&t);
    }
}
