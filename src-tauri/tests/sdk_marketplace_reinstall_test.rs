//! SDK-only reproducer: no client adapter, hooks, UI, remote service, or machine config.
//! Each case runs three independent, committed local Git repositories.
use a2c_smcp::smcp_computer::{
    computer::{Computer, SilentSession},
    settings::{
        AddMarketplaceParams, EnvMap, InstallOptions, RemoveMarketplaceParams, UninstallOptions,
    },
};
use std::{fs, path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(repo)
        .args(args)
        .output()
        .expect("run fixture Git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn write_origin(repo: &Path) {
    fs::create_dir_all(repo.join(".tfrobot-plugin")).unwrap();
    fs::create_dir_all(repo.join("plugin/.tfrobot-plugin")).unwrap();
    fs::create_dir_all(repo.join("plugin/skills/example")).unwrap();
    fs::write(
        repo.join(".tfrobot-plugin/marketplace.json"),
        r#"{"plugins":[{"name":"probe","source":"./plugin"}]}"#,
    )
    .unwrap();
    fs::write(
        repo.join("plugin/.tfrobot-plugin/plugin.json"),
        r#"{"name":"probe","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        repo.join("plugin/skills/example/SKILL.md"),
        "---\nname: example\ndescription: fixture skill\n---\nfixture\n",
    )
    .unwrap();
    git(repo, &["init", "-q"]);
    git(repo, &["add", "."]);
    git(
        repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-qm",
            "fixture",
        ],
    );
}

#[derive(Clone, Copy, Debug)]
enum Reinstall {
    Default,
    SyncSource,
    RefreshInstall,
    RecreateSource,
}

async fn check_reinstall(case: Reinstall) {
    let mut failures = Vec::new();
    for sample in 1..=3 {
        let scratch = tempfile::tempdir().unwrap();
        let origin = scratch.path().join("origin");
        write_origin(&origin);
        let home = scratch.path().join("skill-home");
        let anchor = scratch.path().join("config");
        fs::create_dir_all(&anchor).unwrap();
        let env = EnvMap::from([("XDG_CONFIG_HOME".to_string(), anchor.display().to_string())]);
        let computer = Computer::new(
            "reinstall-probe",
            SilentSession::new("fixture"),
            None,
            None,
            false,
            false,
        )
        .with_skill_home(&home)
        .with_config_dir(&anchor)
        .with_config_env(env)
        .with_blob_cache_root(scratch.path().join("blob"));
        let url = url::Url::from_file_path(&origin).unwrap().to_string();
        computer
            .add_marketplace(
                &url,
                AddMarketplaceParams {
                    name: Some("fixture"),
                    ..Default::default()
                },
            )
            .await
            .expect("add committed fixture source");
        let installed = computer
            .install_plugin(
                "probe@fixture",
                InstallOptions {
                    scope: Some("user"),
                    ..Default::default()
                },
                None,
            )
            .await
            .expect("initial install must succeed");
        let root = std::path::PathBuf::from(installed.install_path.expect("installed path"));
        assert!(
            root.starts_with(&home),
            "fixture must not point to the origin"
        );
        assert!(root.join("skills/example/SKILL.md").is_file());
        assert!(computer
            .uninstall_plugin(
                "probe@fixture",
                UninstallOptions {
                    scope: Some("user"),
                    ..Default::default()
                },
                None
            )
            .await
            .expect("uninstall succeeds"));
        let root_after_uninstall = root.exists();
        let deleted_paths = git(&home.join("marketplace/fixture"), &["status", "--short"])
            .lines()
            .filter(|line| line.starts_with(" D "))
            .count();
        assert!(
            origin.join("plugin/skills/example/SKILL.md").is_file(),
            "origin must be preserved"
        );
        assert!(
            git(&origin, &["status", "--short"]).is_empty(),
            "origin stays clean"
        );
        match case {
            Reinstall::SyncSource => {
                let rows = computer.refresh_marketplace("fixture").await;
                println!("case={case:?} sample={sample} refresh={rows:?}");
            }
            Reinstall::RecreateSource => {
                computer
                    .remove_marketplace(
                        "fixture",
                        RemoveMarketplaceParams {
                            keep_plugins: true,
                            hooks: None,
                        },
                    )
                    .await
                    .expect("remove source after uninstall");
                computer
                    .add_marketplace(
                        &url,
                        AddMarketplaceParams {
                            name: Some("fixture"),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("recreate source");
            }
            _ => {}
        }
        let result = computer
            .install_plugin(
                "probe@fixture",
                InstallOptions {
                    scope: Some("user"),
                    refresh: matches!(case, Reinstall::RefreshInstall),
                    ..Default::default()
                },
                None,
            )
            .await;
        let error = result.as_ref().err().map(ToString::to_string);
        println!("case={case:?} sample={sample} root_after_uninstall={root_after_uninstall} deleted_tracked_paths={deleted_paths} reinstall_ok={} error={error:?}", result.is_ok());
        if let Some(error) = error {
            failures.push(format!("{case:?} sample={sample}: {error}"));
        } else {
            let recovered_root =
                std::path::PathBuf::from(result.unwrap().install_path.expect("reinstalled path"));
            assert!(
                recovered_root.join("skills/example/SKILL.md").is_file(),
                "successful reinstall restores contents"
            );
            let inventory = computer.governance_snapshot().await.unwrap();
            assert!(inventory
                .plugins
                .iter()
                .any(|plugin| plugin.id == "probe@fixture" && plugin.installed && !plugin.enabled));
        }
    }
    assert!(
        failures.is_empty(),
        "uninstall/reinstall must work: {failures:#?}"
    );
}

#[tokio::test]
#[ignore = "Known upstream defect: https://github.com/A2C-SMCP/rust-sdk/issues/271; run with --include-ignored when validating an SDK upgrade"]
async fn sdk_relative_source_reinstalls_after_uninstall() {
    check_reinstall(Reinstall::Default).await;
}

#[tokio::test]
#[ignore = "Known upstream defect: https://github.com/A2C-SMCP/rust-sdk/issues/271; run with --include-ignored when validating an SDK upgrade"]
async fn sdk_relative_source_reinstalls_after_source_sync() {
    check_reinstall(Reinstall::SyncSource).await;
}

#[tokio::test]
#[ignore = "Known upstream defect: https://github.com/A2C-SMCP/rust-sdk/issues/271; run with --include-ignored when validating an SDK upgrade"]
async fn sdk_relative_source_reinstalls_with_explicit_refresh() {
    check_reinstall(Reinstall::RefreshInstall).await;
}

#[tokio::test]
async fn sdk_relative_source_reinstalls_after_source_recreation() {
    check_reinstall(Reinstall::RecreateSource).await;
}
