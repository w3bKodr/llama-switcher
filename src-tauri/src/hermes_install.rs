//! Detect a Hermes Agent skills directory and install the bundled adapter into
//! it. Shared by the GUI "Install skill to Hermes" button. The NSIS installer
//! performs the equivalent step at install time.

use std::path::{Path, PathBuf};

/// Candidate Hermes skills directories for installed Hermes profiles.
/// Hermes discovers `SKILL.md` files recursively below `<HERMES_HOME>/skills`.
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];

    if let Ok(dir) = std::env::var("HERMES_SKILLS_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(home) = std::env::var("HERMES_HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("skills"));
    }

    if let Ok(profile) = std::env::var("USERPROFILE") {
        let hermes_home = PathBuf::from(profile).join(".hermes");
        if hermes_home.is_dir() {
            candidates.push(hermes_home.join("skills"));

            // Named profiles use their own Hermes home and skills directory.
            let profiles = hermes_home.join("profiles");
            if let Ok(entries) = std::fs::read_dir(profiles) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        candidates.push(entry.path().join("skills"));
                    }
                }
            }
        }
    }

    // De-duplicate. A Hermes home is sufficient evidence even when its skills
    // directory has not been created yet.
    let mut seen: Vec<PathBuf> = vec![];
    candidates
        .into_iter()
        .filter(|p| p.is_dir() || p.parent().is_some_and(Path::is_dir))
        .filter(|p| {
            if seen.contains(p) {
                false
            } else {
                seen.push(p.clone());
                true
            }
        })
        .collect()
}

fn normalize_skills_dir(selected: &Path) -> PathBuf {
    if selected.file_name().is_some_and(|name| name.eq_ignore_ascii_case("skills")) {
        selected.to_path_buf()
    } else if selected.join("config.yaml").exists()
        || selected.file_name().is_some_and(|name| name.eq_ignore_ascii_case(".hermes"))
    {
        selected.join("skills")
    } else {
        selected.to_path_buf()
    }
}

fn upsert_env_value(contents: &mut String, key: &str, value: &str) {
    let prefix = format!("{}=", key);
    let replacement = format!("{}={}", key, value);
    let mut found = false;
    let mut lines: Vec<String> = contents
        .lines()
        .map(|line| {
            if line.trim_start().starts_with(&prefix) {
                found = true;
                replacement.clone()
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        lines.push(replacement);
    }
    *contents = lines.join("\n");
    contents.push('\n');
}

fn copy_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name();
            // Never copy node_modules or a developer's real .env.
            if name == "node_modules" || name == ".env" {
                continue;
            }
            copy_path(&entry.path(), &dst.join(name))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Copy the native Hermes skill into `<HERMES_HOME>/skills/llama-switcher` and
/// place its credentials in `<HERMES_HOME>/.env`, where Hermes securely loads
/// required environment variables for skills.
pub fn install(
    source_root: &Path,
    skills_dir: &Path,
    base_url: &str,
    token: &str,
) -> Result<PathBuf, String> {
    if !source_root.join("SKILL.md").exists() {
        return Err(format!(
            "Skill source not found at {}.",
            source_root.display()
        ));
    }
    let skills_dir = normalize_skills_dir(skills_dir);
    if !skills_dir.is_dir() {
        std::fs::create_dir_all(&skills_dir)
            .map_err(|e| format!("Cannot create skills dir: {}", e))?;
    }

    let dest = skills_dir.join("llama-switcher");
    let items = ["SKILL.md", "scripts", "README.md"];
    for item in items {
        let src = source_root.join(item);
        if src.exists() {
            copy_path(&src, &dest.join(item))
                .map_err(|e| format!("Failed to copy {}: {}", item, e))?;
        }
    }

    let hermes_home = skills_dir.parent().ok_or_else(|| {
        format!("Cannot determine Hermes home from {}.", skills_dir.display())
    })?;
    let env_path = hermes_home.join(".env");
    let mut env_contents = std::fs::read_to_string(&env_path).unwrap_or_default();
    upsert_env_value(&mut env_contents, "LLAMA_SWITCHER_BASE_URL", base_url);
    upsert_env_value(&mut env_contents, "LLAMA_SWITCHER_API_TOKEN", token);
    std::fs::write(&env_path, env_contents)
        .map_err(|e| format!("Failed to update {}: {}", env_path.display(), e))?;

    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_native_skill_and_updates_profile_env() {
        let root = std::env::temp_dir().join(format!(
            "llama-switcher-hermes-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        let home = root.join(".hermes");
        std::fs::create_dir_all(source.join("scripts")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: llama-switcher\n---\n").unwrap();
        std::fs::write(source.join("scripts/client.py"), "print('ok')\n").unwrap();
        std::fs::write(home.join(".env"), "EXISTING=value\nLLAMA_SWITCHER_API_TOKEN=old\n").unwrap();

        let dest = install(&source, &home, "http://127.0.0.1:1234", "new-token").unwrap();

        assert_eq!(dest, home.join("skills/llama-switcher"));
        assert!(dest.join("SKILL.md").is_file());
        assert!(dest.join("scripts/client.py").is_file());
        let env = std::fs::read_to_string(home.join(".env")).unwrap();
        assert!(env.contains("EXISTING=value"));
        assert!(env.contains("LLAMA_SWITCHER_BASE_URL=http://127.0.0.1:1234"));
        assert!(env.contains("LLAMA_SWITCHER_API_TOKEN=new-token"));
        assert!(!env.contains("LLAMA_SWITCHER_API_TOKEN=old"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
