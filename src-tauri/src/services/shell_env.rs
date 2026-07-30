#[cfg(not(target_os = "windows"))]
use std::path::Path;
use std::sync::OnceLock;

/// Cached auto-detected PATH from login shell.
static DETECTED_PATH: OnceLock<String> = OnceLock::new();

/// Resolve the user's full PATH from their login shell and set it as the
/// process environment variable. Call this in `main()` before any threads.
///
/// Strategy (macOS/Linux only, no-op on Windows):
/// 1. Spawn the user's login shell to get their configured PATH
/// 2. If that fails, prepend well-known directories to the existing PATH
pub fn fix_path_env() {
    #[cfg(target_os = "windows")]
    return;

    #[cfg(not(target_os = "windows"))]
    {
        let new_path = match resolve_path_from_shell() {
            Some(path) => path,
            None => fallback_path(),
        };

        DETECTED_PATH.set(new_path.clone()).ok();
        std::env::set_var("PATH", &new_path);
    }
}

/// Return the auto-detected PATH (cached from startup).
/// Used by the frontend as a default/placeholder value.
pub fn get_detected_path() -> String {
    DETECTED_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default())
}

/// Resolve PATH from the user's login shell.
/// Runs `$SHELL -l -c 'printf "%s" "$PATH"'` with a 5-second timeout.
#[cfg(not(target_os = "windows"))]
fn resolve_path_from_shell() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let child = std::process::Command::new(&shell)
        .args(["-l", "-c", r#"printf "%s" "$PATH""#])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let output = child.wait_with_output();
        let _ = tx.send(output);
    });

    let result = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .ok()?
        .ok()?;

    let _ = handle.join();

    if !result.status.success() {
        return None;
    }

    let path = String::from_utf8(result.stdout).ok()?.trim().to_string();

    // Sanity check: must contain /usr/bin and be non-empty
    if path.is_empty() || !path.contains("/usr/bin") {
        return None;
    }

    Some(path)
}

/// Build a fallback PATH by prepending well-known directories to the existing PATH.
/// Only includes directories that actually exist on disk.
#[cfg(not(target_os = "windows"))]
fn fallback_path() -> String {
    let current = std::env::var("PATH").unwrap_or_default();

    let mut extra: Vec<String> = Vec::new();

    if let Some(home) = dirs::home_dir() {
        let candidates = [home.join(".local/bin"), home.join(".cargo/bin")];
        for p in &candidates {
            if p.exists() {
                extra.push(p.to_string_lossy().to_string());
            }
        }
    }

    let system_candidates = ["/usr/local/bin", "/opt/homebrew/bin", "/opt/homebrew/sbin"];
    for p in &system_candidates {
        if Path::new(p).exists() && !current.contains(p) {
            extra.push(p.to_string());
        }
    }

    if extra.is_empty() {
        return current;
    }

    let mut parts: Vec<&str> = extra.iter().map(|s| s.as_str()).collect();
    for entry in current.split(':') {
        if !entry.is_empty() && !parts.contains(&entry) {
            parts.push(entry);
        }
    }

    parts.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fix_path_env_no_panic() {
        fix_path_env();
        let path = std::env::var("PATH").unwrap_or_default();
        assert!(!path.is_empty());
    }

    #[test]
    fn test_get_detected_path_returns_non_empty() {
        let path = get_detected_path();
        assert!(!path.is_empty());
    }

    #[cfg(not(target_os = "windows"))]
    mod unix_tests {
        use super::*;

        #[test]
        fn test_resolve_path_from_shell_succeeds() {
            let result = resolve_path_from_shell();
            assert!(result.is_some(), "login shell should return a PATH");
            let path = result.unwrap();
            assert!(path.contains("/usr/bin"), "PATH should contain /usr/bin");
        }

        #[test]
        fn test_fallback_path_preserves_original() {
            let current = std::env::var("PATH").unwrap_or_default();
            let result = fallback_path();
            // All original PATH entries should still be present
            for entry in current.split(':') {
                if !entry.is_empty() {
                    assert!(
                        result.contains(entry),
                        "fallback should preserve original entry: {}",
                        entry
                    );
                }
            }
        }

        #[test]
        fn test_fallback_path_includes_existing_dirs() {
            let result = fallback_path();
            if std::path::Path::new("/usr/local/bin").exists() {
                assert!(
                    result.contains("/usr/local/bin"),
                    "fallback should include /usr/local/bin"
                );
            }
        }

        #[test]
        fn test_fallback_path_skips_nonexistent() {
            // Nonexistent paths should not appear
            let result = fallback_path();
            assert!(
                !result.contains("/this/path/definitely/does/not/exist"),
                "fallback should not include nonexistent paths"
            );
        }
    }
}
