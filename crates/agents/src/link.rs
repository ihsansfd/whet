use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use whet_core::config::Paths;

/// Points a harness's rules file at whet's rules. An existing file is imported
/// when whet has no rules yet, and backed up either way.
pub fn link_rules(target: &Path, paths: &Paths) -> Result<Vec<String>> {
    let mut log = Vec::new();
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }

    if let Ok(meta) = fs::symlink_metadata(target) {
        let is_link = meta.file_type().is_symlink();
        if is_link {
            let dest = fs::read_link(target)?;
            let dest = target.parent().map_or(dest.clone(), |d| d.join(&dest));
            if dest == paths.rules {
                return Ok(vec![format!(
                    "{} already points at {}",
                    target.display(),
                    paths.rules.display()
                )]);
            }
        }
        // A dangling link has nothing to import or back up.
        let content = fs::read_to_string(target).unwrap_or_default();
        let whet_has_rules = fs::read_to_string(&paths.rules).is_ok_and(|r| !r.trim().is_empty());
        if !content.trim().is_empty() && !whet_has_rules {
            fs::create_dir_all(&paths.home)?;
            fs::write(&paths.rules, &content)?;
            log.push(format!(
                "imported {} into {}",
                target.display(),
                paths.rules.display()
            ));
        }
        if !is_link && !content.trim().is_empty() {
            let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            let backup = format!("{}.bak-{millis}", target.display());
            fs::copy(target, &backup)?;
            log.push(format!("backed up {} to {backup}", target.display()));
        }
        fs::remove_file(target)?;
    }

    symlink(&paths.rules, target)?;
    log.push(format!(
        "linked {} → {}",
        target.display(),
        paths.rules.display()
    ));
    Ok(log)
}

#[cfg(unix)]
fn symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(original, link)
}
