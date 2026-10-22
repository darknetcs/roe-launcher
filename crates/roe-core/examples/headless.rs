//! End-to-end check of the core without a GUI: update from GitHub, prepare, translate, build the command.
use roe_core::*;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(std::env::args().nth(1).expect("usage: headless <server-dir>"));
    let sink: Sink = Arc::new(|l| println!("  | {l}"));

    println!("1) GitHub: latest commit");
    let remote = update::remote_head()?;
    println!("   {} {} ({})", &remote.sha[..10], remote.message, remote.date);

    let local = update::read_info(&dir);
    println!("2) installed: {:?}  update available: {}", local.as_ref().map(|i| &i.sha[..10]), update::update_available(local.as_ref(), &remote));
    if update::update_available(local.as_ref(), &remote) || !update::is_installed(&dir) {
        update::install(&dir, &remote.sha, &sink)?;
    }

    println!("3) runtime (python venv + deps + dotnet)");
    let quiet: Sink = Arc::new(|l: String| if l.contains("Installing") || l.contains("Creating") || l.contains("Successfully") { println!("  | {l}") });
    let prepared = runtime::ensure(&dir, &quiet)?;
    println!("   python={} dotnet={}", prepared.python.display(), prepared.dotnet.display());

    println!("4) translate");
    let rep = translate::apply(&dir, &prepared.python)?;
    println!("   files changed: {}  untranslated: {}", rep.files_changed, rep.untranslated);
    let mut info = update::read_info(&dir).unwrap_or_default();
    info.translated = true;
    info.untranslated = Some(rep.untranslated);
    update::write_info(&dir, &info)?;

    println!("5) server command");
    let s = server::Settings { dir: dir.clone(), game_dir: "/game/Ring of Elysium".into(), duration: 1500, extra_args: String::new(), lan_tunnel: false, options: roe_core::options::to_args(&roe_core::options::launcher_defaults(), None), profile: "default".into() };
    let cmd = server::build_command(&prepared, &s);
    println!("   {} {}", cmd.get_program().to_string_lossy(), cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>().join(" "));
    println!("OK");
    Ok(())
}
