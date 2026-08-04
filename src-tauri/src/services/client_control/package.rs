use a2c_smcp::smcp_computer::skills::{parse_skill_frontmatter, FORBIDDEN_SKILL_FILES};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Weak};
use tokio::sync::Mutex;

const MAX_SKILL_FILES: usize = 256;
const MAX_SKILL_MD_BYTES: usize = 1024 * 1024;
const MAX_RESOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 16 * 1024 * 1024;
const TRANSACTIONS_DIR: &str = ".client_control_transactions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillContentEncoding {
    Utf8,
    Base64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillFileInput {
    pub path: String,
    pub encoding: SkillContentEncoding,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkillFileChange {
    Upsert {
        path: String,
        encoding: SkillContentEncoding,
        content: String,
    },
    Delete {
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRefreshStatus {
    Pending,
    Notified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMutationResult {
    pub name: String,
    pub source: String,
    pub revision: Option<String>,
    pub previous_revision: Option<String>,
    pub changed_paths: Vec<String>,
    pub refresh_status: SkillRefreshStatus,
}

impl SkillMutationResult {
    pub fn mark_notified(&mut self) {
        self.refresh_status = SkillRefreshStatus::Notified;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillPackageErrorCode {
    InvalidSkillName,
    InvalidPath,
    ForbiddenFile,
    InvalidEncoding,
    InvalidPackage,
    PackageLimitExceeded,
    SkillAlreadyExists,
    SkillNotFound,
    RevisionConflict,
    RestartRequired,
    UnsafeExistingPackage,
    MutationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct SkillPackageError {
    pub code: SkillPackageErrorCode,
    pub message: String,
}

impl SkillPackageError {
    fn new(code: SkillPackageErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn io(operation: &str, error: impl std::fmt::Display) -> Self {
        Self::new(
            SkillPackageErrorCode::MutationFailed,
            format!("{operation} failed: {error}"),
        )
    }
}

#[derive(Default)]
pub struct SkillPackageService {
    mutation_locks: std::sync::Mutex<HashMap<String, Weak<Mutex<()>>>>,
}

impl SkillPackageService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Completes or rolls back Client Control Skill transactions left by a previous process.
    /// This is called during application startup before SDK Computer runtimes are constructed, so
    /// the SDK can never observe a half-committed package tree.
    pub fn recover_home(
        &self,
        configured_home: &Path,
        effective_home: &Path,
    ) -> Result<(), SkillPackageError> {
        let roots = package_roots(configured_home, effective_home)?;
        recover_all_transactions(&roots)
    }

    pub async fn create(
        &self,
        configured_home: PathBuf,
        effective_home: PathBuf,
        name: String,
        files: Vec<SkillFileInput>,
    ) -> Result<SkillMutationResult, SkillPackageError> {
        validate_skill_name(&name)?;
        let lock = self.mutation_lock(&effective_home, &name);
        let _guard = lock.lock().await;
        tokio::task::spawn_blocking(move || {
            create_package(&configured_home, &effective_home, &name, files)
        })
        .await
        .map_err(|error| SkillPackageError::io("join create mutation", error))?
    }

    pub async fn update(
        &self,
        configured_home: PathBuf,
        effective_home: PathBuf,
        name: String,
        changes: Vec<SkillFileChange>,
        expected_revision: Option<String>,
    ) -> Result<SkillMutationResult, SkillPackageError> {
        validate_skill_name(&name)?;
        let lock = self.mutation_lock(&effective_home, &name);
        let _guard = lock.lock().await;
        tokio::task::spawn_blocking(move || {
            update_package(
                &configured_home,
                &effective_home,
                &name,
                changes,
                expected_revision.as_deref(),
            )
        })
        .await
        .map_err(|error| SkillPackageError::io("join update mutation", error))?
    }

    pub async fn delete(
        &self,
        configured_home: PathBuf,
        effective_home: PathBuf,
        name: String,
        expected_revision: Option<String>,
    ) -> Result<SkillMutationResult, SkillPackageError> {
        validate_skill_name(&name)?;
        let lock = self.mutation_lock(&effective_home, &name);
        let _guard = lock.lock().await;
        tokio::task::spawn_blocking(move || {
            delete_package(
                &configured_home,
                &effective_home,
                &name,
                expected_revision.as_deref(),
            )
        })
        .await
        .map_err(|error| SkillPackageError::io("join delete mutation", error))?
    }

    fn mutation_lock(&self, home: &Path, name: &str) -> Arc<Mutex<()>> {
        let key = format!("{}\0{name}", home.display());
        let mut locks = self
            .mutation_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        locks.retain(|_, lock| lock.strong_count() > 0);
        let lock = Arc::new(Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }
}

struct PackageRoots {
    home: PathBuf,
    user: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct TransactionMetadata {
    name: String,
    operation: String,
}

fn package_roots(
    configured_home: &Path,
    effective_home: &Path,
) -> Result<PackageRoots, SkillPackageError> {
    let configured = absolute_identity(configured_home)?;
    let effective = absolute_identity(effective_home)?;
    if configured != effective {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::RestartRequired,
            "configured and effective Skill Home differ; restart the Computer before mutation",
        ));
    }
    create_dir_private(effective_home)?;
    let user = effective_home.join("user");
    create_dir_private(&user)?;
    let home = fs::canonicalize(effective_home)
        .map_err(|error| SkillPackageError::io("resolve effective Skill Home", error))?;
    let user = fs::canonicalize(&user)
        .map_err(|error| SkillPackageError::io("resolve user Skill root", error))?;
    if !user.starts_with(&home) {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidPath,
            "user Skill root escapes effective Skill Home",
        ));
    }
    Ok(PackageRoots { home, user })
}

fn create_package(
    configured_home: &Path,
    effective_home: &Path,
    name: &str,
    files: Vec<SkillFileInput>,
) -> Result<SkillMutationResult, SkillPackageError> {
    let roots = package_roots(configured_home, effective_home)?;
    recover_transactions(&roots, name)?;
    let destination = roots.user.join(name);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::SkillAlreadyExists,
            format!("User Skill already exists: {name}"),
        ));
    }
    let transaction = begin_transaction(&roots.home, name, "create")?;
    let candidate = transaction.join("candidate");
    create_dir_private(&candidate)?;
    let result = (|| {
        write_initial_files(&candidate, files)?;
        let revision = validate_package_tree(&candidate, name)?;
        fs::rename(&candidate, &destination)
            .map_err(|error| SkillPackageError::io("commit Skill create", error))?;
        Ok(SkillMutationResult {
            name: name.to_string(),
            source: "user".to_string(),
            revision: Some(revision),
            previous_revision: None,
            changed_paths: list_relative_files(&destination)?,
            refresh_status: SkillRefreshStatus::Pending,
        })
    })();
    cleanup_transaction(&transaction);
    result
}

fn update_package(
    configured_home: &Path,
    effective_home: &Path,
    name: &str,
    changes: Vec<SkillFileChange>,
    expected_revision: Option<&str>,
) -> Result<SkillMutationResult, SkillPackageError> {
    update_package_with_rename(
        configured_home,
        effective_home,
        name,
        changes,
        expected_revision,
        |from, to| fs::rename(from, to),
    )
}

fn update_package_with_rename<F>(
    configured_home: &Path,
    effective_home: &Path,
    name: &str,
    changes: Vec<SkillFileChange>,
    expected_revision: Option<&str>,
    mut rename: F,
) -> Result<SkillMutationResult, SkillPackageError>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let roots = package_roots(configured_home, effective_home)?;
    recover_transactions(&roots, name)?;
    let current = resolve_existing_user_package(&roots.user, name)?;
    let previous_revision = validate_package_tree(&current, name)?;
    require_revision(expected_revision, &previous_revision)?;
    let transaction = begin_transaction(&roots.home, name, "update")?;
    let candidate = transaction.join("candidate");
    create_dir_private(&candidate)?;
    let backup = transaction.join("previous");
    let mut recovery_required = false;
    let result = (|| {
        copy_regular_tree(&current, &candidate)?;
        let changed_paths = apply_changes(&candidate, changes)?;
        let revision = validate_package_tree(&candidate, name)?;
        rename(&current, &backup)
            .map_err(|error| SkillPackageError::io("quarantine previous Skill package", error))?;
        if let Err(error) = rename(&candidate, &current) {
            let rollback = rename(&backup, &current);
            return Err(match rollback {
                Ok(()) => SkillPackageError::io("commit Skill update", error),
                Err(rollback_error) => {
                    recovery_required = true;
                    SkillPackageError::new(
                        SkillPackageErrorCode::MutationFailed,
                        format!(
                            "commit Skill update failed: {error}; rollback failed: {rollback_error}; recovery transaction retained at {}",
                            transaction.display()
                        ),
                    )
                }
            });
        }
        Ok(SkillMutationResult {
            name: name.to_string(),
            source: "user".to_string(),
            revision: Some(revision),
            previous_revision: Some(previous_revision),
            changed_paths,
            refresh_status: SkillRefreshStatus::Pending,
        })
    })();
    if !recovery_required {
        cleanup_transaction(&transaction);
    }
    result
}

fn delete_package(
    configured_home: &Path,
    effective_home: &Path,
    name: &str,
    expected_revision: Option<&str>,
) -> Result<SkillMutationResult, SkillPackageError> {
    let roots = package_roots(configured_home, effective_home)?;
    recover_transactions(&roots, name)?;
    let current = resolve_existing_user_package(&roots.user, name)?;
    let previous_revision = validate_package_tree(&current, name)?;
    require_revision(expected_revision, &previous_revision)?;
    let changed_paths = list_relative_files(&current)?;
    let transaction = begin_transaction(&roots.home, name, "delete")?;
    let quarantine = transaction.join("deleted");
    let result = fs::rename(&current, &quarantine)
        .map_err(|error| SkillPackageError::io("commit Skill delete", error))
        .map(|()| SkillMutationResult {
            name: name.to_string(),
            source: "user".to_string(),
            revision: None,
            previous_revision: Some(previous_revision),
            changed_paths,
            refresh_status: SkillRefreshStatus::Pending,
        });
    // The atomic rename is the delete commit point. Cleanup is intentionally best-effort: a
    // quarantined package is outside every SDK source and can be recovered/cleaned on startup.
    cleanup_transaction(&transaction);
    result
}

fn validate_skill_name(name: &str) -> Result<(), SkillPackageError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !name.contains("--");
    if valid {
        Ok(())
    } else {
        Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidSkillName,
            "Skill name must be lowercase kebab-case and at most 64 characters",
        ))
    }
}

fn validate_relative_path(value: &str) -> Result<PathBuf, SkillPackageError> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains(':')
        || value.split('/').any(str::is_empty)
    {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidPath,
            format!("invalid Skill resource path: {value:?}"),
        ));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
        })
    {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidPath,
            format!("Skill resource path must be a normalized POSIX relative path: {value:?}"),
        ));
    }
    if path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        FORBIDDEN_SKILL_FILES
            .iter()
            .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
    }) {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::ForbiddenFile,
            "Skill package contains a forbidden credential file",
        ));
    }
    Ok(path.to_path_buf())
}

fn decode_content(
    encoding: SkillContentEncoding,
    content: &str,
) -> Result<Vec<u8>, SkillPackageError> {
    match encoding {
        SkillContentEncoding::Utf8 => Ok(content.as_bytes().to_vec()),
        SkillContentEncoding::Base64 => base64::engine::general_purpose::STANDARD
            .decode(content)
            .map_err(|_| {
                SkillPackageError::new(
                    SkillPackageErrorCode::InvalidEncoding,
                    "Skill resource contains invalid base64",
                )
            }),
    }
}

fn write_initial_files(root: &Path, files: Vec<SkillFileInput>) -> Result<(), SkillPackageError> {
    let mut paths = BTreeSet::new();
    let mut folded_nodes = HashMap::<String, String>::new();
    let mut prepared = Vec::with_capacity(files.len());
    for file in files {
        let relative = validate_relative_path(&file.path)?;
        let normalized = path_string(&relative);
        if !paths.insert(normalized.clone()) {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::InvalidPackage,
                format!("duplicate resource path: {normalized}"),
            ));
        }
        let mut prefix = PathBuf::new();
        for component in relative.components() {
            prefix.push(component.as_os_str());
            let original = path_string(&prefix);
            let key = original.to_lowercase();
            if let Some(existing) = folded_nodes.get(&key) {
                if existing != &original {
                    return Err(SkillPackageError::new(
                        SkillPackageErrorCode::InvalidPackage,
                        format!("case-folding resource conflict: {existing} vs {original}"),
                    ));
                }
            } else {
                folded_nodes.insert(key, original);
            }
        }
        prepared.push((relative, decode_content(file.encoding, &file.content)?));
    }
    for (relative, content) in prepared {
        write_resource(root, &relative, content)?;
    }
    Ok(())
}

fn apply_changes(
    root: &Path,
    changes: Vec<SkillFileChange>,
) -> Result<Vec<String>, SkillPackageError> {
    let mut changed = BTreeSet::new();
    for change in changes {
        match change {
            SkillFileChange::Upsert {
                path,
                encoding,
                content,
            } => {
                let relative = validate_relative_path(&path)?;
                let normalized = path_string(&relative);
                write_resource(root, &relative, decode_content(encoding, &content)?)?;
                changed.insert(normalized);
            }
            SkillFileChange::Delete { path } => {
                let relative = validate_relative_path(&path)?;
                if relative == Path::new("SKILL.md") {
                    return Err(SkillPackageError::new(
                        SkillPackageErrorCode::InvalidPackage,
                        "SKILL.md cannot be deleted",
                    ));
                }
                let destination = root.join(&relative);
                let metadata = fs::symlink_metadata(&destination).map_err(|_| {
                    SkillPackageError::new(
                        SkillPackageErrorCode::InvalidPath,
                        format!("Skill resource does not exist: {}", path_string(&relative)),
                    )
                })?;
                if !metadata.is_file() {
                    return Err(SkillPackageError::new(
                        SkillPackageErrorCode::UnsafeExistingPackage,
                        format!(
                            "Skill resource is not a regular file: {}",
                            path_string(&relative)
                        ),
                    ));
                }
                fs::remove_file(&destination)
                    .map_err(|error| SkillPackageError::io("delete Skill resource", error))?;
                changed.insert(path_string(&relative));
            }
        }
    }
    Ok(changed.into_iter().collect())
}

fn write_resource(root: &Path, relative: &Path, bytes: Vec<u8>) -> Result<(), SkillPackageError> {
    let limit = if relative == Path::new("SKILL.md") {
        MAX_SKILL_MD_BYTES
    } else {
        MAX_RESOURCE_BYTES
    };
    if bytes.len() > limit {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::PackageLimitExceeded,
            format!(
                "Skill resource exceeds its decoded size limit: {}",
                path_string(relative)
            ),
        ));
    }
    let destination = root.join(relative);
    if let Some(parent) = destination.parent() {
        create_dir_private(parent)?;
    }
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::UnsafeExistingPackage,
                format!(
                    "refusing to replace non-regular resource: {}",
                    path_string(relative)
                ),
            ));
        }
    }
    write_file_private(&destination, &bytes)
}

fn validate_package_tree(root: &Path, name: &str) -> Result<String, SkillPackageError> {
    let files = list_regular_files(root)?;
    if files.len() > MAX_SKILL_FILES {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::PackageLimitExceeded,
            format!("Skill package exceeds {MAX_SKILL_FILES} files"),
        ));
    }
    let mut folded = HashMap::<String, String>::new();
    let mut total = 0usize;
    let mut has_skill_md = false;
    let mut digest = Sha256::new();
    for (relative, absolute, size) in &files {
        let path = path_string(relative);
        let mut prefix = PathBuf::new();
        for component in relative.components() {
            prefix.push(component.as_os_str());
            let original = path_string(&prefix);
            let key = original.to_lowercase();
            if let Some(existing) = folded.get(&key) {
                if existing != &original {
                    return Err(SkillPackageError::new(
                        SkillPackageErrorCode::InvalidPackage,
                        format!("case-folding resource conflict: {existing} vs {original}"),
                    ));
                }
            } else {
                folded.insert(key, original);
            }
        }
        let per_file_limit = if relative == Path::new("SKILL.md") {
            has_skill_md = true;
            MAX_SKILL_MD_BYTES
        } else {
            MAX_RESOURCE_BYTES
        };
        if *size > per_file_limit {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::PackageLimitExceeded,
                format!("Skill resource exceeds its decoded size limit: {path}"),
            ));
        }
        total = total.checked_add(*size).ok_or_else(|| {
            SkillPackageError::new(
                SkillPackageErrorCode::PackageLimitExceeded,
                "Skill package size overflow",
            )
        })?;
        if total > MAX_PACKAGE_BYTES {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::PackageLimitExceeded,
                format!("Skill package exceeds {MAX_PACKAGE_BYTES} decoded bytes"),
            ));
        }
        let bytes = fs::read(absolute)
            .map_err(|error| SkillPackageError::io("read Skill resource", error))?;
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(&bytes);
    }
    if !has_skill_md {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidPackage,
            "Skill package must contain SKILL.md",
        ));
    }
    validate_skill_markdown(&root.join("SKILL.md"), name)?;
    Ok(hex::encode(digest.finalize()))
}

fn validate_skill_markdown(path: &Path, name: &str) -> Result<(), SkillPackageError> {
    let bytes = fs::read(path).map_err(|error| SkillPackageError::io("read SKILL.md", error))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        SkillPackageError::new(
            SkillPackageErrorCode::InvalidPackage,
            "SKILL.md must be UTF-8",
        )
    })?;
    let frontmatter = parse_skill_frontmatter(text);
    let description = frontmatter
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|description| !description.is_empty());
    if description.is_none() {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidPackage,
            "SKILL.md requires valid frontmatter with a non-empty description",
        ));
    }
    if let Some(declared_name) = frontmatter.get("name") {
        if declared_name.as_str() != Some(name) {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::InvalidPackage,
                "SKILL.md frontmatter name must match the package name",
            ));
        }
    }
    Ok(())
}

fn resolve_existing_user_package(
    user_root: &Path,
    name: &str,
) -> Result<PathBuf, SkillPackageError> {
    let path = user_root.join(name);
    let metadata = fs::symlink_metadata(&path).map_err(|_| {
        SkillPackageError::new(
            SkillPackageErrorCode::SkillNotFound,
            format!("User Skill does not exist: {name}"),
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::UnsafeExistingPackage,
            "User Skill package is not a physical directory",
        ));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| SkillPackageError::io("resolve User Skill package", error))?;
    if !canonical.starts_with(user_root) {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::UnsafeExistingPackage,
            "User Skill package escapes the user root",
        ));
    }
    Ok(canonical)
}

fn list_regular_files(root: &Path) -> Result<Vec<(PathBuf, PathBuf, usize)>, SkillPackageError> {
    let mut files = Vec::new();
    visit_regular_tree(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn visit_regular_tree(
    root: &Path,
    current: &Path,
    files: &mut Vec<(PathBuf, PathBuf, usize)>,
) -> Result<(), SkillPackageError> {
    for entry in fs::read_dir(current)
        .map_err(|error| SkillPackageError::io("enumerate Skill package", error))?
    {
        let entry = entry.map_err(|error| SkillPackageError::io("read Skill entry", error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| SkillPackageError::io("inspect Skill entry", error))?;
        if metadata.file_type().is_symlink() {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::UnsafeExistingPackage,
                "Skill package contains a symbolic link",
            ));
        }
        if metadata.is_dir() {
            visit_regular_tree(root, &path, files)?;
        } else if metadata.is_file() {
            if has_multiple_hard_links(&path, &metadata) {
                return Err(SkillPackageError::new(
                    SkillPackageErrorCode::UnsafeExistingPackage,
                    "Skill package contains a hard-linked file",
                ));
            }
            let relative = path.strip_prefix(root).map_err(|_| {
                SkillPackageError::new(
                    SkillPackageErrorCode::InvalidPath,
                    "Skill resource escapes package root",
                )
            })?;
            validate_relative_path(&path_string(relative))?;
            files.push((relative.to_path_buf(), path, metadata.len() as usize));
        } else {
            return Err(SkillPackageError::new(
                SkillPackageErrorCode::UnsafeExistingPackage,
                "Skill package contains a special file",
            ));
        }
    }
    Ok(())
}

fn copy_regular_tree(source: &Path, destination: &Path) -> Result<(), SkillPackageError> {
    for (relative, absolute, _) in list_regular_files(source)? {
        let bytes = fs::read(&absolute)
            .map_err(|error| SkillPackageError::io("read existing Skill resource", error))?;
        write_resource(destination, &relative, bytes)?;
    }
    Ok(())
}

fn list_relative_files(root: &Path) -> Result<Vec<String>, SkillPackageError> {
    Ok(list_regular_files(root)?
        .into_iter()
        .map(|(relative, _, _)| path_string(&relative))
        .collect())
}

fn require_revision(expected: Option<&str>, actual: &str) -> Result<(), SkillPackageError> {
    if expected.is_some_and(|expected| expected != actual) {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::RevisionConflict,
            "Skill package revision changed",
        ));
    }
    Ok(())
}

fn begin_transaction(
    home: &Path,
    name: &str,
    operation: &str,
) -> Result<PathBuf, SkillPackageError> {
    let transactions = home.join(TRANSACTIONS_DIR);
    create_dir_private(&transactions)?;
    let transaction = transactions.join(uuid::Uuid::new_v4().to_string());
    create_dir_private(&transaction)?;
    let metadata = serde_json::to_vec(&TransactionMetadata {
        name: name.to_string(),
        operation: operation.to_string(),
    })
    .map_err(|error| SkillPackageError::io("encode Skill transaction metadata", error))?;
    write_file_private(&transaction.join("transaction.json"), &metadata)?;
    Ok(transaction)
}

fn recover_transactions(roots: &PackageRoots, name: &str) -> Result<(), SkillPackageError> {
    let transactions = roots.home.join(TRANSACTIONS_DIR);
    let Ok(entries) = fs::read_dir(&transactions) else {
        return Ok(());
    };
    let destination = roots.user.join(name);
    for entry in entries {
        let entry =
            entry.map_err(|error| SkillPackageError::io("read Skill transaction", error))?;
        let transaction = entry.path();
        let metadata = fs::symlink_metadata(&transaction)
            .map_err(|error| SkillPackageError::io("inspect Skill transaction", error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let recorded_name = fs::read(transaction.join("transaction.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<TransactionMetadata>(&bytes).ok())
            .map(|metadata| metadata.name);
        let previous = transaction.join("previous");
        let legacy_matches = recorded_name.is_none()
            && previous.is_dir()
            && validate_package_tree(&previous, name).is_ok();
        if recorded_name.as_deref() != Some(name) && !legacy_matches {
            continue;
        }

        if previous.is_dir() && fs::symlink_metadata(&destination).is_err() {
            fs::rename(&previous, &destination)
                .map_err(|error| SkillPackageError::io("recover previous Skill package", error))?;
        }
        // A `deleted` quarantine means the delete rename reached its commit point. It must never
        // be restored; removing the transaction completes the already-committed delete.
        cleanup_transaction(&transaction);
    }
    Ok(())
}

fn recover_all_transactions(roots: &PackageRoots) -> Result<(), SkillPackageError> {
    let transactions = roots.home.join(TRANSACTIONS_DIR);
    let Ok(entries) = fs::read_dir(&transactions) else {
        return Ok(());
    };
    let mut names = BTreeSet::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| SkillPackageError::io("read Skill transaction", error))?;
        let transaction = entry.path();
        match fs::symlink_metadata(&transaction) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            _ => continue,
        }
        let Some(metadata) = fs::read(transaction.join("transaction.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<TransactionMetadata>(&bytes).ok())
        else {
            // Unknown entries are retained for manual inspection. Startup recovery only mutates
            // directories that carry metadata written by this implementation.
            continue;
        };
        if validate_skill_name(&metadata.name).is_err()
            || !matches!(metadata.operation.as_str(), "create" | "update" | "delete")
        {
            log::warn!(
                "retaining unrecognized Client Control Skill transaction {} for manual inspection",
                transaction.display()
            );
            continue;
        }
        names.insert(metadata.name);
    }
    for name in names {
        recover_transactions(roots, &name)?;
    }
    Ok(())
}

fn cleanup_transaction(transaction: &Path) {
    if transaction.exists() {
        if let Err(error) = fs::remove_dir_all(transaction) {
            log::warn!(
                "failed to clean Client Control Skill transaction {}: {}",
                transaction.display(),
                error
            );
        }
    }
    if let Some(parent) = transaction.parent() {
        let _ = fs::remove_dir(parent);
    }
}

fn absolute_identity(path: &Path) -> Result<PathBuf, SkillPackageError> {
    if !path.is_absolute() {
        return Err(SkillPackageError::new(
            SkillPackageErrorCode::InvalidPath,
            "Skill Home must be absolute",
        ));
    }
    if path.exists() {
        fs::canonicalize(path).map_err(|error| SkillPackageError::io("resolve Skill Home", error))
    } else {
        Ok(path.to_path_buf())
    }
}

fn path_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn create_dir_private(path: &Path) -> Result<(), SkillPackageError> {
    fs::create_dir_all(path)
        .map_err(|error| SkillPackageError::io("create private Skill directory", error))?;
    set_dir_private(path)
}

fn write_file_private(path: &Path, bytes: &[u8]) -> Result<(), SkillPackageError> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| SkillPackageError::io("write private Skill resource", error))?;
    file.write_all(bytes)
        .map_err(|error| SkillPackageError::io("write private Skill resource", error))?;
    file.sync_all()
        .map_err(|error| SkillPackageError::io("sync private Skill resource", error))?;
    set_file_private(path)
}

#[cfg(unix)]
fn set_dir_private(path: &Path) -> Result<(), SkillPackageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| SkillPackageError::io("set Skill directory permissions", error))
}

#[cfg(not(unix))]
fn set_dir_private(_path: &Path) -> Result<(), SkillPackageError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_private(path: &Path) -> Result<(), SkillPackageError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| SkillPackageError::io("set Skill resource permissions", error))
}

#[cfg(not(unix))]
fn set_file_private(_path: &Path) -> Result<(), SkillPackageError> {
    Ok(())
}

#[cfg(unix)]
fn has_multiple_hard_links(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() > 1
}

#[cfg(windows)]
fn has_multiple_hard_links(path: &Path, _metadata: &fs::Metadata) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let Ok(file) = fs::File::open(path) else {
        return true;
    };
    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `file` owns a valid handle for the duration of the call and `information` points to
    // writable storage of the exact structure required by the Win32 API.
    let succeeded =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) != 0 };
    if !succeeded {
        return true;
    }
    // SAFETY: the Win32 call succeeded and initialized the complete output structure.
    unsafe { information.assume_init().nNumberOfLinks > 1 }
}

#[cfg(not(any(unix, windows)))]
fn has_multiple_hard_links(_path: &Path, _metadata: &fs::Metadata) -> bool {
    // This client currently ships on Unix and Windows. Unknown targets must fail closed until a
    // reliable file-identity implementation is provided for that platform.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn skill_md(name: &str, body: &str) -> SkillFileInput {
        SkillFileInput {
            path: "SKILL.md".to_string(),
            encoding: SkillContentEncoding::Utf8,
            content: format!("---\nname: {name}\ndescription: Test package\n---\n{body}\n"),
        }
    }

    #[tokio::test]
    async fn create_update_delete_roundtrip_preserves_unmentioned_resources() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let service = SkillPackageService::new();
        let created = service
            .create(
                home.clone(),
                home.clone(),
                "sample-skill".to_string(),
                vec![
                    skill_md("sample-skill", "v1"),
                    SkillFileInput {
                        path: "references/guide.txt".to_string(),
                        encoding: SkillContentEncoding::Utf8,
                        content: "keep".to_string(),
                    },
                    SkillFileInput {
                        path: "assets/data.bin".to_string(),
                        encoding: SkillContentEncoding::Base64,
                        content: base64::engine::general_purpose::STANDARD.encode([0, 1, 2, 3]),
                    },
                ],
            )
            .await
            .unwrap();
        assert!(created.revision.is_some());

        let updated = service
            .update(
                home.clone(),
                home.clone(),
                "sample-skill".to_string(),
                vec![SkillFileChange::Upsert {
                    path: "SKILL.md".to_string(),
                    encoding: SkillContentEncoding::Utf8,
                    content: "---\nname: sample-skill\ndescription: Test package\n---\nv2\n"
                        .to_string(),
                }],
                created.revision.clone(),
            )
            .await
            .unwrap();
        assert_ne!(created.revision, updated.revision);
        assert_eq!(
            fs::read_to_string(home.join("user/sample-skill/references/guide.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read(home.join("user/sample-skill/assets/data.bin")).unwrap(),
            vec![0, 1, 2, 3]
        );

        let deleted = service
            .delete(
                home.clone(),
                home.clone(),
                "sample-skill".to_string(),
                updated.revision.clone(),
            )
            .await
            .unwrap();
        assert!(deleted.revision.is_none());
        assert!(!home.join("user/sample-skill").exists());
    }

    #[tokio::test]
    async fn rejects_restart_mismatch_traversal_forbidden_and_invalid_frontmatter() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let service = SkillPackageService::new();
        let mismatch = service
            .create(
                home.clone(),
                temp.path().join("effective"),
                "sample-skill".to_string(),
                vec![skill_md("sample-skill", "body")],
            )
            .await
            .unwrap_err();
        assert_eq!(mismatch.code, SkillPackageErrorCode::RestartRequired);

        for path in ["../escape", "nested\\escape", ".skillenv"] {
            let error = service
                .create(
                    home.clone(),
                    home.clone(),
                    format!("bad-{}", uuid::Uuid::new_v4().simple()),
                    vec![
                        skill_md("placeholder", "body"),
                        SkillFileInput {
                            path: path.to_string(),
                            encoding: SkillContentEncoding::Utf8,
                            content: "secret".to_string(),
                        },
                    ],
                )
                .await
                .unwrap_err();
            assert!(matches!(
                error.code,
                SkillPackageErrorCode::InvalidPath | SkillPackageErrorCode::ForbiddenFile
            ));
        }

        let invalid = service
            .create(
                home.clone(),
                home,
                "invalid-frontmatter".to_string(),
                vec![SkillFileInput {
                    path: "SKILL.md".to_string(),
                    encoding: SkillContentEncoding::Utf8,
                    content: "# missing frontmatter".to_string(),
                }],
            )
            .await
            .unwrap_err();
        assert_eq!(invalid.code, SkillPackageErrorCode::InvalidPackage);
    }

    #[tokio::test]
    async fn revision_conflict_does_not_overwrite_local_edit() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let service = SkillPackageService::new();
        let created = service
            .create(
                home.clone(),
                home.clone(),
                "sample-skill".to_string(),
                vec![skill_md("sample-skill", "v1")],
            )
            .await
            .unwrap();
        fs::write(
            home.join("user/sample-skill/SKILL.md"),
            "---\nname: sample-skill\ndescription: local\n---\nlocal\n",
        )
        .unwrap();
        let error = service
            .update(
                home.clone(),
                home.clone(),
                "sample-skill".to_string(),
                vec![SkillFileChange::Upsert {
                    path: "SKILL.md".to_string(),
                    encoding: SkillContentEncoding::Utf8,
                    content: "---\nname: sample-skill\ndescription: remote\n---\nremote\n"
                        .to_string(),
                }],
                created.revision,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, SkillPackageErrorCode::RevisionConflict);
        assert!(fs::read_to_string(home.join("user/sample-skill/SKILL.md"))
            .unwrap()
            .contains("local"));
    }

    #[tokio::test]
    async fn rejects_case_folded_ancestor_conflicts() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let error = SkillPackageService::new()
            .create(
                home.clone(),
                home,
                "case-conflict".to_string(),
                vec![
                    skill_md("case-conflict", "body"),
                    SkillFileInput {
                        path: "References/a.txt".to_string(),
                        encoding: SkillContentEncoding::Utf8,
                        content: "a".to_string(),
                    },
                    SkillFileInput {
                        path: "references/b.txt".to_string(),
                        encoding: SkillContentEncoding::Utf8,
                        content: "b".to_string(),
                    },
                ],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, SkillPackageErrorCode::InvalidPackage);
        assert!(!temp.path().join("skills/user/case-conflict").exists());
    }

    #[test]
    fn recovers_interrupted_update_and_finishes_committed_delete() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let roots = package_roots(&home, &home).unwrap();
        let destination = roots.user.join("recovered-skill");
        create_dir_private(&destination).unwrap();
        write_initial_files(&destination, vec![skill_md("recovered-skill", "previous")]).unwrap();

        let interrupted = begin_transaction(&roots.home, "recovered-skill", "update").unwrap();
        let previous = interrupted.join("previous");
        fs::rename(&destination, &previous).unwrap();
        SkillPackageService::new()
            .recover_home(&home, &home)
            .unwrap();
        assert!(destination.join("SKILL.md").is_file());
        assert!(!interrupted.exists());

        let committed_delete = begin_transaction(&roots.home, "recovered-skill", "delete").unwrap();
        fs::rename(&destination, committed_delete.join("deleted")).unwrap();
        recover_transactions(&roots, "recovered-skill").unwrap();
        assert!(!destination.exists());
        assert!(!committed_delete.exists());
    }

    #[test]
    fn rollback_failure_retains_previous_package_for_startup_recovery() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let roots = package_roots(&home, &home).unwrap();
        let destination = roots.user.join("recoverable-skill");
        create_dir_private(&destination).unwrap();
        write_initial_files(
            &destination,
            vec![skill_md("recoverable-skill", "previous")],
        )
        .unwrap();

        let mut rename_count = 0usize;
        let error = update_package_with_rename(
            &home,
            &home,
            "recoverable-skill",
            vec![SkillFileChange::Upsert {
                path: "SKILL.md".to_string(),
                encoding: SkillContentEncoding::Utf8,
                content: "---\nname: recoverable-skill\ndescription: test\n---\nnext\n".to_string(),
            }],
            None,
            |from, to| {
                rename_count += 1;
                if rename_count == 1 {
                    fs::rename(from, to)
                } else {
                    Err(std::io::Error::other("injected rename failure"))
                }
            },
        )
        .unwrap_err();
        assert!(error.message.contains("recovery transaction retained"));
        assert!(!destination.exists());
        let transactions = roots.home.join(TRANSACTIONS_DIR);
        assert_eq!(fs::read_dir(&transactions).unwrap().count(), 1);

        SkillPackageService::new()
            .recover_home(&home, &home)
            .unwrap();
        assert!(destination.join("SKILL.md").is_file());
        assert!(fs::read_to_string(destination.join("SKILL.md"))
            .unwrap()
            .contains("previous"));
        assert!(!transactions.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn update_rejects_existing_symlink_without_following_it() {
        use std::os::unix::fs::symlink;
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let package = home.join("user/sample-skill");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("SKILL.md"),
            "---\nname: sample-skill\ndescription: test\n---\nbody\n",
        )
        .unwrap();
        symlink(temp.path().join("outside"), package.join("escape")).unwrap();
        let error = SkillPackageService::new()
            .update(
                home.clone(),
                home,
                "sample-skill".to_string(),
                Vec::new(),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, SkillPackageErrorCode::UnsafeExistingPackage);
    }

    #[tokio::test]
    async fn update_rejects_existing_hard_link() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("skills");
        let package = home.join("user/sample-skill");
        fs::create_dir_all(&package).unwrap();
        let skill_path = package.join("SKILL.md");
        fs::write(
            &skill_path,
            "---\nname: sample-skill\ndescription: test\n---\nbody\n",
        )
        .unwrap();
        fs::hard_link(&skill_path, temp.path().join("outside-link")).unwrap();

        let error = SkillPackageService::new()
            .update(
                home.clone(),
                home,
                "sample-skill".to_string(),
                Vec::new(),
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, SkillPackageErrorCode::UnsafeExistingPackage);
    }
}
