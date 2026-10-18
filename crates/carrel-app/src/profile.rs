//! Explicit native rehearsal profiles never fall back to the installed archive.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{AppError, Result, config::InstallPaths};

const PROFILE_FLAG: &str = "--isolated-profile";

pub(crate) struct IsolatedProfile {
    root: PathBuf,
}

impl IsolatedProfile {
    pub(crate) fn resolve() -> Result<Option<Self>> {
        let Some(path) = parse_profile_argument(env::args_os().skip(1))? else {
            return Ok(None);
        };
        require_supported_platform()?;
        Self::prepare(&path).map(Some)
    }

    fn prepare(path: &Path) -> Result<Self> {
        validate_profile_path(path)?;
        create_directory(path)?;
        let root = path.canonicalize().map_err(|error| {
            AppError::Config(format!(
                "could not resolve isolated profile directory: {error}"
            ))
        })?;
        validate_profile_path(&root)?;
        require_private_directory(&root)?;
        for name in ["data", "config", "webview"] {
            let directory = root.join(name);
            create_directory(&directory)?;
            require_private_directory(&directory)?;
        }
        Ok(Self { root })
    }

    pub(crate) fn paths(&self) -> InstallPaths {
        InstallPaths::for_profile(&self.root)
    }

    pub(crate) fn create_main_window(&self, app: &tauri::App) -> Result<()> {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == "main")
            .ok_or_else(|| {
                AppError::Config("isolated profile requires the main window configuration".into())
            })?;
        let builder = tauri::WebviewWindowBuilder::from_config(app, config)?.incognito(false);
        // The pinned runtime omits the storage fields when converting WindowConfig.
        #[cfg(target_os = "macos")]
        let builder = builder.data_store_identifier(webview_id(&self.root));
        #[cfg(not(target_os = "macos"))]
        let builder = builder.data_directory(self.root.join("webview"));
        builder.build()?;
        Ok(())
    }
}

fn parse_profile_argument(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<PathBuf>> {
    let mut arguments = arguments.into_iter();
    let mut profile = None;
    while let Some(argument) = arguments.next() {
        if argument == PROFILE_FLAG {
            if profile.is_some() {
                return Err(AppError::Config(
                    "--isolated-profile may be specified only once".into(),
                ));
            }
            let path = arguments.next().map(PathBuf::from).ok_or_else(|| {
                AppError::Config("--isolated-profile requires an absolute directory path".into())
            })?;
            validate_profile_path(&path)?;
            profile = Some(path);
        } else if argument
            .to_str()
            .is_some_and(|value| value.starts_with("--isolated-profile="))
        {
            return Err(AppError::Config(
                "use --isolated-profile followed by an absolute directory path".into(),
            ));
        }
    }
    Ok(profile)
}

fn validate_profile_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.parent().is_none()
        || path.as_os_str().as_encoded_bytes().contains(&0)
    {
        return Err(AppError::Config(
            "--isolated-profile requires an absolute directory path below the filesystem root"
                .into(),
        ));
    }
    Ok(())
}

fn create_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|error| {
        AppError::Config(format!(
            "could not create isolated profile directory: {error}"
        ))
    })
}

fn require_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::Config(format!(
            "could not inspect isolated profile directory: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::Config(
            "isolated profile directories must be real directories".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(AppError::Config(
                "isolated profile directories must be accessible only to their owner".into(),
            ));
        }
    }
    Ok(())
}

fn require_supported_platform() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .map_err(|error| {
                AppError::Config(format!(
                    "could not determine macOS version for isolated profile: {error}"
                ))
            })?;
        if !output.status.success()
            || !std::str::from_utf8(&output.stdout).is_ok_and(macos_supports_profile)
        {
            // Wry otherwise silently selects the production default store before macOS 14.
            return Err(AppError::Config(
                "isolated profiles require a known macOS version of 14 or later".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn macos_supports_profile(version: &str) -> bool {
    let components: Vec<_> = version.trim().split('.').collect();
    if components.is_empty()
        || components.len() > 3
        || components.iter().any(|component| {
            component.is_empty()
                || !component.bytes().all(|byte| byte.is_ascii_digit())
                || component.parse::<u32>().is_err()
        })
    {
        return false;
    }
    components[0].parse::<u32>().is_ok_and(|major| major >= 14)
}

#[cfg(any(target_os = "macos", test))]
fn webview_id(canonical_root: &Path) -> [u8; 16] {
    let mut hasher =
        blake3::Hasher::new_derive_key("net.carrel.reader isolated profile webview v1");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(canonical_root.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in canonical_root.as_os_str().encode_wide() {
            hasher.update(&unit.to_le_bytes());
        }
    }
    #[cfg(not(any(unix, windows)))]
    hasher.update(canonical_root.as_os_str().as_encoded_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(|value| OsString::from(*value)).collect()
    }

    #[test]
    fn normal_launch_arguments_do_not_select_a_profile() {
        assert!(parse_profile_argument(Vec::new()).unwrap().is_none());
        assert!(
            parse_profile_argument(arguments(&["carrel://today", "--unrelated"]))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn explicit_absolute_profile_is_preserved_including_spaces_and_unicode() {
        let path = env::temp_dir().join("Carrel rehearsal 日本語");
        let args = vec![OsString::from(PROFILE_FLAG), path.clone().into_os_string()];
        assert_eq!(parse_profile_argument(args).unwrap(), Some(path));
    }

    #[test]
    fn malformed_profile_arguments_never_fall_back_to_production() {
        for args in [
            arguments(&[PROFILE_FLAG]),
            arguments(&[PROFILE_FLAG, ""]),
            arguments(&[PROFILE_FLAG, "relative/profile"]),
            arguments(&["--isolated-profile=/tmp/profile"]),
        ] {
            assert!(parse_profile_argument(args).is_err());
        }
        let path = env::temp_dir()
            .join("carrel-duplicate-profile")
            .into_os_string();
        assert!(
            parse_profile_argument(vec![
                PROFILE_FLAG.into(),
                path.clone(),
                PROFILE_FLAG.into(),
                path
            ])
            .is_err()
        );
        assert!(validate_profile_path(Path::new("/")).is_err());
        assert!(validate_profile_path(Path::new("/tmp/invalid\0profile")).is_err());
    }

    #[test]
    fn macos_version_parser_accepts_supported_numeric_versions_only() {
        for version in ["14", "14.0", "14.0.1\n", " 15.7.3 ", "27.0.1"] {
            assert!(macos_supports_profile(version), "{version:?}");
        }
        for version in [
            "",
            "13.9.9",
            "10.16",
            "unknown",
            "14.beta",
            "14.",
            "14..1",
            "14.0.0.1",
            "-14.0",
            "14.0 extra",
            "4294967296.0",
            "１４.0",
        ] {
            assert!(!macos_supports_profile(version), "{version:?}");
        }
    }

    #[test]
    fn profile_identity_survives_key_replacement_and_separates_profiles() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("profile");
        let first = IsolatedProfile::prepare(&path).unwrap();
        let id = webview_id(&first.root);
        let keys = first.paths().keys;
        fs::create_dir(&keys).unwrap();
        fs::write(keys.join("device"), "old fixture key").unwrap();
        fs::remove_dir_all(&keys).unwrap();
        fs::create_dir(&keys).unwrap();
        fs::write(keys.join("device"), "replacement fixture key").unwrap();
        let reopened = IsolatedProfile::prepare(&path).unwrap();
        assert_eq!(id, webview_id(&reopened.root));
        let other = IsolatedProfile::prepare(&parent.path().join("other")).unwrap();
        assert_ne!(id, webview_id(&other.root));
        let raw = blake3::hash(first.root.as_os_str().as_encoded_bytes());
        assert_ne!(id.as_slice(), &raw.as_bytes()[..16]);
    }

    #[test]
    fn profile_refuses_files_in_place_of_directories() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("file");
        fs::write(&root, "fixture").unwrap();
        assert!(IsolatedProfile::prepare(&root).is_err());
        let profile = IsolatedProfile::prepare(&parent.path().join("profile")).unwrap();
        fs::remove_dir(profile.root.join("config")).unwrap();
        fs::write(profile.root.join("config"), "fixture").unwrap();
        assert!(IsolatedProfile::prepare(&profile.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unix_profile_directories_are_private_and_aliases_have_the_same_identity() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let parent = tempfile::tempdir().unwrap();
        let profile = IsolatedProfile::prepare(&parent.path().join("profile")).unwrap();
        for directory in [
            &profile.root,
            &profile.root.join("data"),
            &profile.root.join("config"),
            &profile.root.join("webview"),
        ] {
            assert_eq!(
                fs::metadata(directory).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        let alias = parent.path().join("alias");
        symlink(&profile.root, &alias).unwrap();
        let aliased = IsolatedProfile::prepare(&alias).unwrap();
        assert_eq!(webview_id(&profile.root), webview_id(&aliased.root));
        fs::set_permissions(profile.root.join("data"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(IsolatedProfile::prepare(&profile.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unix_profile_refuses_public_existing_roots_without_changing_them() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("public");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(IsolatedProfile::prepare(&root).is_err());
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn unix_profile_paths_preserve_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent
            .path()
            .join(OsString::from_vec(b"profile-\xff".to_vec()));
        let parsed =
            parse_profile_argument(vec![PROFILE_FLAG.into(), root.clone().into_os_string()])
                .unwrap()
                .unwrap();
        assert_eq!(parsed, root);
        assert_ne!(
            webview_id(&parsed),
            webview_id(&PathBuf::from(root.to_string_lossy().into_owned()))
        );

        // Some Unix filesystems reject invalid UTF-8 names. Parsing and hashing
        // must preserve the bytes; preparing a directory must honor the filesystem.
        if let Err(error) = fs::create_dir(&root) {
            let expected = format!("could not create isolated profile directory: {error}");
            assert!(matches!(
                IsolatedProfile::prepare(&parsed),
                Err(AppError::Config(message)) if message == expected
            ));
            assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
            return;
        }
        fs::remove_dir(&root).unwrap();
        let profile = IsolatedProfile::prepare(&parsed).unwrap();
        assert_eq!(profile.root, root.canonicalize().unwrap());
        assert_eq!(
            webview_id(&profile.root),
            webview_id(&root.canonicalize().unwrap())
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_profile_refuses_symlinked_state_directories_without_writing_through_them() {
        use std::os::unix::fs::symlink;

        let parent = tempfile::tempdir().unwrap();
        let profile = IsolatedProfile::prepare(&parent.path().join("profile")).unwrap();
        let unrelated = parent.path().join("unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), "untouched fixture").unwrap();
        fs::remove_dir(profile.root.join("data")).unwrap();
        symlink(&unrelated, profile.root.join("data")).unwrap();
        assert!(IsolatedProfile::prepare(&profile.root).is_err());
        assert_eq!(
            fs::read_to_string(unrelated.join("keep")).unwrap(),
            "untouched fixture"
        );
        assert_eq!(fs::read_dir(unrelated).unwrap().count(), 1);
    }
}
