//! Budowanie kontenera: kroki `build.chroot` w chroot Leap (te same biblioteki co obraz),
//! potem skrypty `build.install` do DESTDIR. Zastępuje dawne tools/build-setup.sh.

use crate::fsx;
use crate::graph::{self, Member};
use crate::kiwi::Installed;
use crate::model::Repo;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Opts {
    /// Adres repozytorium Leap (do zbudowania chroot).
    pub repo_url: Option<String>,
    pub buildroot: PathBuf,
    pub dry_run: bool,
}

/// Wykonuje polecenia albo (dry run) tylko je wypisuje.
struct Runner {
    dry: bool,
}

impl Runner {
    fn run(&self, cmd: &mut Command) -> Result<(), String> {
        let shown = format!("{cmd:?}");
        println!("+ {shown}");
        if self.dry {
            return Ok(());
        }
        let st = cmd.status().map_err(|e| format!("{shown}: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("{shown}: {st}"))
        }
    }
    fn ok(&self, cmd: &mut Command) {
        let _ = self.run(cmd);
    }
}

/// Odmontowuje /dev, /proc i /sys z chroot, także po błędzie.
struct Mounts<'a> {
    br: &'a Path,
    dry: bool,
}

impl Drop for Mounts<'_> {
    fn drop(&mut self) {
        if self.dry {
            return;
        }
        for m in ["proc", "dev", "sys"] {
            let _ = Command::new("umount").arg("-l").arg(self.br.join(m)).status();
        }
    }
}

const SKIP: &[&str] = &["target", "node_modules", "dist", "screens"];

/// Pakiety chroot: `packages.build` boxów z krokiem chroot i ich zależności.
pub fn chroot_packages(repo: &Repo, members: &[Member]) -> Result<Vec<String>, String> {
    let mut set = BTreeSet::new();
    for m in members.iter().filter(|m| repo.boxes[&m.id].file.build.chroot.is_some()) {
        for b in graph::closure(repo, &m.id)? {
            set.extend(repo.get(&b)?.file.packages.build.iter().cloned());
        }
    }
    Ok(set.into_iter().collect())
}

/// Buduje kontener do `dest`. Zwraca pliki, które położył `install.sh` każdego boxa (nowe ścieżki w `dest`).
pub fn build(repo: &Repo, container: &str, dest: &Path, opts: &Opts) -> Result<Installed, String> {
    let members = graph::order(repo, container)?;
    let run = Runner { dry: opts.dry_run };
    let chrooted: Vec<&Member> = members.iter().filter(|m| repo.boxes[&m.id].file.build.chroot.is_some()).collect();

    if !chrooted.is_empty() {
        let br = &opts.buildroot;
        let pkgs = chroot_packages(repo, &members)?;
        let stamp = br.join(".shrimp-box-packages");
        let want = format!("{}\n{}\n", opts.repo_url.as_deref().unwrap_or(""), pkgs.join("\n"));
        if fs::read_to_string(&stamp).ok().as_deref() != Some(want.as_str()) {
            let url = opts.repo_url.as_deref().ok_or("kroki chroot potrzebują --repo URL (repozytorium Leap)")?;
            println!("== chroot Leap: {} ({} pakietów)", br.display(), pkgs.len());
            run.ok(Command::new("rm").arg("-rf").arg(br));
            run.run(Command::new("mkdir").arg("-p").arg(br))?;
            run.run(Command::new("zypper").args(["-n", "--root"]).arg(br).args(["ar", "-f", url, "leap"]))?;
            run.run(Command::new("zypper").args(["-n", "--root"]).arg(br).args(["--gpg-auto-import-keys", "ref"]))?;
            run.run(
                Command::new("zypper")
                    .args(["-n", "--root"])
                    .arg(br)
                    .args(["--gpg-auto-import-keys", "in", "--no-recommends"])
                    .args(&pkgs),
            )?;
            if !opts.dry_run {
                fs::write(&stamp, &want).map_err(|e| e.to_string())?;
            }
        }
        run.run(Command::new("cp").args(["-L", "/etc/resolv.conf"]).arg(br.join("etc/resolv.conf")))?;
        let _mounts = Mounts { br, dry: opts.dry_run };
        run.run(Command::new("mount").args(["--bind", "/dev"]).arg(br.join("dev")))?;
        run.run(Command::new("mount").args(["-t", "proc", "proc"]).arg(br.join("proc")))?;
        run.run(Command::new("mount").args(["-t", "sysfs", "sys"]).arg(br.join("sys")))?;

        // źródła: każdy box z krokiem chroot i jego zależności, w /src/boxes/<id> (jak w repo)
        let mut needed = BTreeSet::new();
        for m in &chrooted {
            needed.extend(graph::closure(repo, &m.id)?);
        }
        let src = br.join("src/boxes");
        run.ok(Command::new("rm").arg("-rf").arg(br.join("src")));
        for id in &needed {
            let b = repo.get(id)?;
            println!("+ kopiuj {} → {}", b.dir.display(), src.join(id).display());
            if !opts.dry_run {
                fsx::copy_tree(&repo.root.join(&b.dir), &src.join(id), SKIP)?;
            }
        }
        for m in &chrooted {
            let b = repo.get(&m.id)?;
            let cmd = b.file.build.chroot.as_deref().unwrap_or_default();
            println!("== box {}: {cmd}", m.id);
            let script = format!(
                "export HOME=/root CARGO_HOME=/root/.cargo RUSTUP_HOME=/root/.rustup\n\
                 if command -v rustup >/dev/null; then rustup toolchain list | grep -q stable || rustup default stable; fi\n\
                 cd /src/boxes/{} && {cmd}",
                m.id
            );
            run.run(Command::new("chroot").arg(br).args(["/bin/bash", "-euxc", &script]))?;
            for o in &b.file.build.outputs {
                let from = src.join(&m.id).join(o);
                let to = repo.root.join(&b.dir).join(o);
                println!("+ kopiuj {} → {}", from.display(), to.display());
                if !opts.dry_run {
                    fsx::copy_file(&from, &to)?;
                }
                // biblioteki programu → pakiety, które obraz musi mieć (dla [packages] image)
                let inside = format!("/src/boxes/{}/{o}", m.id);
                let report = format!(
                    "ldd {inside} 2>/dev/null | awk '/=>/ {{print $3}}' | xargs -r rpm -qf --qf '%{{NAME}}\\n' 2>/dev/null | sort -u | sed 's/^/  /'"
                );
                println!("{} — pakiety bibliotek:", m.id);
                run.ok(Command::new("chroot").arg(br).args(["/bin/bash", "-c", &report]));
            }
        }
    }

    let mut installed = Installed::new();
    for m in &members {
        let b = repo.get(&m.id)?;
        let Some(script) = &b.file.build.install else { continue };
        let before: BTreeSet<PathBuf> = if opts.dry_run { BTreeSet::new() } else { fsx::files(dest)?.into_iter().collect() };
        println!("== box {}: {script} {}", m.id, m.args.join(" "));
        let mut cmd = Command::new(repo.root.join(&b.dir).join(script));
        cmd.arg(dest).args(&m.args).current_dir(&repo.root);
        if b.file.build.chroot.is_some() {
            cmd.env("SHRIMP_PREBUILT", "1");
        }
        run.run(&mut cmd)?;
        if !opts.dry_run {
            let new = fsx::files(dest)?.into_iter().filter(|p| !before.contains(p));
            installed.insert(m.id.clone(), new.map(|p| format!("/{}", p.to_string_lossy())).collect());
        }
    }
    Ok(installed)
}
