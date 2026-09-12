//! Application filesystem paths - single source of truth for all directory locations.
//!
//! [`AppPaths`] holds every base directory the application needs. It is computed
//! once at init time: [`AppPaths::default()`] for production (from `dirs::*`),
//! [`AppPaths::new_in()`] for tests (from a temp root).
//!
//! All domain code that needs a filesystem path must read it from the injected
//! `AppPaths` instance - never from `dirs::*` free functions directly.

use std::path::{Path, PathBuf};

use crate::app_info::APP_NAME;

/// Browser profile mode: one persistent profile per mode.
///
/// `headless` and `headed` each get their own profile subdir so a single
/// warmed headed profile can be shared by both `web-fetch` and `web-search`
/// without colliding on Chromium's singleton lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserProfileMode {
    /// Headless profile (no visible window).
    Headless,
    /// Headed profile (visible window, manually warmable).
    Headed,
}

impl BrowserProfileMode {
    /// Returns the subdirectory name for this mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Headless => "headless",
            Self::Headed => "headed",
        }
    }
}

/// Application filesystem paths.
///
/// Stores app-specific directories (not platform directories). For production,
/// these are derived from `dirs::*`. For tests, from a single temp root.
///
/// Construct once at init and share via `Services.paths`.
#[derive(Debug, Clone)]
#[expect(clippy::struct_field_names, reason = "names match domain terminology")]
pub struct AppPaths {
    /// `~/.config/jinn` - providers.toml, prompts/, personas/, themes/, jinn.toml
    config_dir: PathBuf,
    /// `~/.local/share/jinn` - sessions.db
    data_dir: PathBuf,
    /// `~/.cache/jinn` - model_cache.json
    cache_dir: PathBuf,
    /// `~/.local/state/jinn` - jinn.log
    ///
    /// Falls back to `data_dir` on platforms where `dirs::state_dir()` returns
    /// `None` (e.g. macOS, Windows).
    state_dir: PathBuf,
    /// `~/` - home directory (skills live at `~/.agents/skills`)
    home_dir: PathBuf,
    /// `/usr/share/jinn` - system-wide defaults (themes, personas, prompts).
    ///
    /// User files in `~/.config/jinn/` override system files of the same name.
    system_data_dir: PathBuf,
}

impl Default for AppPaths {
    fn default() -> Self {
        Self {
            config_dir: dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")),
            data_dir: dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")),
            cache_dir: dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")),
            state_dir: dirs::state_dir()
                .or_else(dirs::data_dir)
                .unwrap_or_else(|| PathBuf::from(".")),
            home_dir: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            system_data_dir: PathBuf::from("/usr/share/jinn"),
        }
    }
}

impl AppPaths {
    /// Creates paths derived from a single root directory (for tests).
    ///
    /// All subdirectories are nested under `root/` so that a single
    /// `TempDir` cleans everything up.
    #[must_use]
    pub fn new_in(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            state_dir: root.join("state"),
            home_dir: root.to_path_buf(),
            system_data_dir: root.join("share"),
        }
    }

    /// Sets the home directory for tests (bounds the project walk).
    #[cfg(feature = "testing")]
    pub fn set_home_dir_for_test(&mut self, home: PathBuf) {
        self.home_dir = home;
    }

    // -- Derived paths -------------------------------------------------------

    /// Session database parent directory (`~/.local/share/jinn`).
    ///
    /// Pass to `SqliteSessionStore::new_in()`.
    #[must_use]
    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join(APP_NAME)
    }

    /// Base directory for persistent browser profiles (`~/.local/share/jinn/browser-profile`).
    ///
    /// Per-mode profiles live under subdirs (`browser-profile/headless`,
    /// `browser-profile/headed`) so a single warmed headed profile is shared by
    /// both `web-fetch` and `web-search` when they select `headed-chrome`.
    /// Use [`Self::browser_profile_dir_for_mode`] to resolve a concrete path.
    #[must_use]
    pub fn browser_profile_base_dir(&self) -> PathBuf {
        self.data_dir.join(APP_NAME).join("browser-profile")
    }

    /// Persistent browser profile for a given mode (`headless` or `headed`).
    #[must_use]
    pub fn browser_profile_dir_for_mode(&self, mode: BrowserProfileMode) -> PathBuf {
        self.browser_profile_base_dir().join(mode.as_str())
    }

    /// Prompt templates directory (`~/.config/jinn/prompts`).
    #[must_use]
    pub fn prompts_dir(&self) -> PathBuf {
        self.config_dir.join(APP_NAME).join("prompts")
    }

    /// Agent skills directory (`~/.agents/skills`).
    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.home_dir.join(".agents").join("skills")
    }

    /// User home directory (`~/`). Bounds the project walk (exclusive of home).
    #[must_use]
    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    /// Personas directory (`~/.config/jinn/personas`).
    #[must_use]
    pub fn personas_dir(&self) -> PathBuf {
        self.config_dir.join(APP_NAME).join("personas")
    }

    /// Subscription credential storage (`~/.config/jinn/auth.json`).
    ///
    /// Deliberately separate from `providers.toml`: provider configuration is
    /// meant to be edited and shared, credentials are not. The file is written
    /// with owner-only permissions.
    #[must_use]
    pub fn auth_path(&self) -> PathBuf {
        self.config_dir.join(APP_NAME).join("auth.json")
    }

    /// Model cache file (`~/.cache/jinn/model_cache.json`).
    #[must_use]
    pub fn cache_path(&self) -> PathBuf {
        self.cache_dir.join(APP_NAME).join("model_cache.json")
    }

    /// Provider config file (`~/.config/jinn/providers.toml`).
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join(APP_NAME).join("providers.toml")
    }

    /// Log file path (`~/.local/state/jinn/jinn.log` on Linux).
    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        self.state_dir
            .join(APP_NAME)
            .join(format!("{APP_NAME}.log"))
    }

    /// App state file (`~/.local/state/jinn/state.toml` on Linux).
    #[must_use]
    pub fn state_file_path(&self) -> PathBuf {
        self.state_dir.join(APP_NAME).join("state.toml")
    }

    /// Themes directory (`~/.config/jinn/themes`).
    #[must_use]
    pub fn themes_dir(&self) -> PathBuf {
        self.config_dir.join(APP_NAME).join("themes")
    }

    /// User preferences file (`~/.config/jinn/jinn.toml`).
    #[must_use]
    pub fn preferences_path(&self) -> PathBuf {
        self.config_dir
            .join(APP_NAME)
            .join(crate::app_info::PREFS_FILE_NAME)
    }

    // -- System data directories (XDG fallback) -----------------------------

    /// System data directory (`/usr/share/jinn`).
    ///
    /// Used as a fallback source for themes, personas, and prompts
    /// when user-specific files are not found.
    #[must_use]
    pub fn system_data_dir(&self) -> &Path {
        &self.system_data_dir
    }

    /// Plugin directory (`~/.local/share/jinn/plugins`) — where
    /// user-installed plugin `.wasm` files live. Relative `wasm` paths in
    /// `[[plugin]]` entries resolve against this directory.
    #[must_use]
    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join(APP_NAME).join("plugins")
    }

    /// Config directory (`~/.config/jinn`).
    #[must_use]
    pub fn app_config_dir(&self) -> PathBuf {
        self.config_dir.join(APP_NAME)
    }

    /// Base config directory (`~/.config`) — parent of all jinn config.
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Base data directory (`~/.local/share`) — parent of all jinn data.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// App data directory (`~/.local/share/jinn`) — plugin payloads,
    /// scratch dirs, and other jinn-owned data live here.
    #[must_use]
    pub fn app_data_dir(&self) -> PathBuf {
        self.data_dir.join(APP_NAME)
    }

    /// System themes directory (`/usr/share/jinn/themes`).
    #[must_use]
    pub fn system_themes_dir(&self) -> PathBuf {
        self.system_data_dir.join("themes")
    }

    /// System prompts directory (`/usr/share/jinn/prompts`).
    #[must_use]
    pub fn system_prompts_dir(&self) -> PathBuf {
        self.system_data_dir.join("prompts")
    }
    /// System skills directory (`/usr/share/jinn/skills`).
    ///
    /// Installed by the system package. Used as a fallback source for
    /// skills when user-global skills are not found.
    #[must_use]
    pub fn system_skills_dir(&self) -> PathBuf {
        self.system_data_dir.join("skills")
    }

    /// User's models.dev reference file (`~/.cache/jinn/models.dev.json`).
    ///
    /// Written by `jinn fetch models`. Contains the full models.dev API
    /// dump used as a fallback for context window lookup.
    #[must_use]
    pub fn models_dev_user_path(&self) -> PathBuf {
        self.cache_dir.join(APP_NAME).join("models.dev.json")
    }

    /// System models.dev reference file (`/usr/share/jinn/models.dev.json`).
    ///
    /// Installed by the system package. Used as a fallback when no
    /// user-level file exists.
    #[must_use]
    pub fn models_dev_system_path(&self) -> PathBuf {
        self.system_data_dir.join("models.dev.json")
    }

    // -- Merged resource paths (system + user) ---------------------------------

    /// Returns merged theme file paths from system and user directories.
    ///
    /// System themes are listed first. If a user theme has the same filename
    /// as a system theme, the user version replaces the system one in the result.
    /// The returned list is sorted by filename stem.
    #[must_use]
    pub fn resolve_theme_paths(&self) -> Vec<(String, PathBuf)> {
        resolve_resource_paths(&self.system_themes_dir(), &self.themes_dir(), "toml")
    }

    /// Returns merged prompt template file paths from system and user directories.
    ///
    /// Same merge semantics as [`resolve_theme_paths`].
    #[must_use]
    pub fn resolve_prompt_paths(&self) -> Vec<(String, PathBuf)> {
        resolve_resource_paths(&self.system_prompts_dir(), &self.prompts_dir(), "md")
    }
}

/// Scans two directories for files with the given extension, merging by filename stem.
///
/// System files are listed first. User files with the same stem replace system ones.
/// Results are sorted by stem.
fn resolve_resource_paths(
    system_dir: &Path,
    user_dir: &Path,
    extension: &str,
) -> Vec<(String, PathBuf)> {
    use std::collections::BTreeMap;

    let mut map = BTreeMap::new();

    // System files first (lower priority).
    scan_dir_into(&mut map, system_dir, extension);
    // User files override system files of the same name.
    scan_dir_into(&mut map, user_dir, extension);

    map.into_iter().collect()
}

/// Scans a directory for files with the given extension and inserts into the map by stem.
fn scan_dir_into(
    map: &mut std::collections::BTreeMap<String, PathBuf>,
    dir: &Path,
    extension: &str,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == extension)
            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
        {
            map.insert(name.to_owned(), path);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing,
        reason = "test code"
    )]
    use super::*;

    #[rstest::rstest]
    fn resolve_theme_paths_returns_empty_when_both_dirs_missing() {
        // Given an AppPaths with nonexistent system and user dirs.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // When resolving theme paths.
        let result = paths.resolve_theme_paths();

        // Then the result is empty.
        assert!(result.is_empty());
    }

    #[rstest::rstest]
    fn resolve_theme_paths_finds_system_files() {
        // Given an AppPaths with a system themes dir containing one theme.
        let root = tempfile::TempDir::new().expect("temp dir");
        let system_dir = root.path().join("share/themes");
        std::fs::create_dir_all(&system_dir).expect("create dir");
        std::fs::write(system_dir.join("ocean.toml"), "focus_accent = \"blue\"").expect("write");

        let paths = AppPaths::new_in(root.path());

        // When resolving theme paths.
        let result = paths.resolve_theme_paths();

        // Then the system theme is found.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "ocean");
    }

    #[rstest::rstest]
    fn resolve_theme_paths_finds_user_files() {
        // Given an AppPaths with a user themes dir containing one theme.
        let root = tempfile::TempDir::new().expect("temp dir");
        let user_dir = root.path().join("config/jinn/themes");
        std::fs::create_dir_all(&user_dir).expect("create dir");
        std::fs::write(user_dir.join("forest.toml"), "focus_accent = \"green\"").expect("write");

        let paths = AppPaths::new_in(root.path());

        // When resolving theme paths.
        let result = paths.resolve_theme_paths();

        // Then the user theme is found.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "forest");
    }

    #[rstest::rstest]
    fn resolve_theme_paths_user_overrides_system() {
        // Given system and user dirs both with a theme named "ocean".
        let root = tempfile::TempDir::new().expect("temp dir");

        let system_dir = root.path().join("share/themes");
        std::fs::create_dir_all(&system_dir).expect("create dir");
        std::fs::write(system_dir.join("ocean.toml"), "focus_accent = \"blue\"").expect("write");

        let user_dir = root.path().join("config/jinn/themes");
        std::fs::create_dir_all(&user_dir).expect("create dir");
        std::fs::write(user_dir.join("ocean.toml"), "focus_accent = \"red\"").expect("write");

        let paths = AppPaths::new_in(root.path());

        // When resolving theme paths.
        let result = paths.resolve_theme_paths();

        // Then only one "ocean" entry exists (user version).
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "ocean");
        // And it points to the user directory.
        assert!(result[0].1.starts_with(user_dir));
    }

    #[rstest::rstest]
    fn resolve_theme_paths_merges_both_dirs() {
        // Given system has "ocean", user has "forest".
        let root = tempfile::TempDir::new().expect("temp dir");

        let system_dir = root.path().join("share/themes");
        std::fs::create_dir_all(&system_dir).expect("create dir");
        std::fs::write(system_dir.join("ocean.toml"), "focus_accent = \"blue\"").expect("write");

        let user_dir = root.path().join("config/jinn/themes");
        std::fs::create_dir_all(&user_dir).expect("create dir");
        std::fs::write(user_dir.join("forest.toml"), "focus_accent = \"green\"").expect("write");

        let paths = AppPaths::new_in(root.path());

        // When resolving theme paths.
        let result = paths.resolve_theme_paths();

        // Then both themes are found, sorted by name.
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "forest");
        assert_eq!(result[1].0, "ocean");
    }

    #[rstest::rstest]
    fn resolve_prompt_paths_uses_md_extension() {
        // Given a system prompts dir with a .md file.
        let root = tempfile::TempDir::new().expect("temp dir");
        let system_dir = root.path().join("share/prompts");
        std::fs::create_dir_all(&system_dir).expect("create dir");
        std::fs::write(system_dir.join("example.md"), "content").expect("write");

        let paths = AppPaths::new_in(root.path());

        // When resolving prompt paths.
        let result = paths.resolve_prompt_paths();

        // Then the .md file is found.
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "example");
    }

    #[rstest::rstest]
    fn default_system_data_dir_is_usr_share() {
        // Given a default AppPaths.
        let paths = AppPaths::default();

        // Then the system data dir is /usr/share/jinn.
        assert_eq!(paths.system_data_dir(), Path::new("/usr/share/jinn"));
    }

    #[rstest::rstest]
    fn new_in_system_data_dir_is_under_root() {
        // Given an AppPaths created with new_in.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // Then the system data dir is root/share.
        assert_eq!(paths.system_data_dir(), root.path().join("share"));
    }

    #[rstest::rstest]
    fn default_system_skills_dir_is_usr_share() {
        // Given a default AppPaths.
        let paths = AppPaths::default();

        // Then the system skills dir is /usr/share/jinn/skills.
        assert_eq!(
            paths.system_skills_dir(),
            Path::new("/usr/share/jinn/skills")
        );
    }

    #[rstest::rstest]
    fn new_in_system_skills_dir_is_under_root() {
        // Given an AppPaths created with new_in.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // Then the system skills dir is root/share/skills.
        assert_eq!(paths.system_skills_dir(), root.path().join("share/skills"));
    }

    #[rstest::rstest]
    fn log_path_under_state_dir_in_new_in() {
        // Given an AppPaths rooted in a temp dir.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // Then log_path is root/state/jinn/jinn.log.
        assert_eq!(
            paths.log_path(),
            root.path().join("state").join("jinn").join("jinn.log")
        );
    }

    #[rstest::rstest]
    fn state_file_path_under_state_dir_in_new_in() {
        // Given an AppPaths rooted in a temp dir.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // Then state_file_path is root/state/jinn/state.toml.
        assert_eq!(
            paths.state_file_path(),
            root.path().join("state").join("jinn").join("state.toml")
        );
    }

    #[rstest::rstest]
    fn log_path_filename_is_app_name_dot_log() {
        // Given an AppPaths.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // Then the filename component is jinn.log.
        let log_path = paths.log_path();
        let filename = log_path.file_name().and_then(|s| s.to_str());
        assert_eq!(filename, Some("jinn.log"));
    }

    #[rstest::rstest]
    fn browser_profile_dir_for_mode_lands_under_data_base() {
        // Given an AppPaths rooted at a temp dir.
        let root = tempfile::TempDir::new().expect("temp dir");
        let paths = AppPaths::new_in(root.path());

        // When resolving the headed profile dir.
        let headed = paths.browser_profile_dir_for_mode(BrowserProfileMode::Headed);
        let headless = paths.browser_profile_dir_for_mode(BrowserProfileMode::Headless);

        // Then both land under data/jinn/browser-profile/<mode>.
        assert_eq!(headed, root.path().join("data/jinn/browser-profile/headed"));
        assert_eq!(
            headless,
            root.path().join("data/jinn/browser-profile/headless")
        );
    }
}
