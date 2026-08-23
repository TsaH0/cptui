//! Configuration loading and XDG paths.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub editors: EditorsConfig,
    #[serde(default)]
    pub companion: CompanionConfig,
    #[serde(default)]
    pub cpp: CppConfig,
    #[serde(default)]
    pub runner: RunnerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorConfig {
    #[serde(default = "default_editor")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            command: default_editor(),
            args: vec![],
        }
    }
}

/// Extra editor commands reachable via dedicated keys (e.g. `v` for neovim).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorsConfig {
    /// `o` key: editor command (e.g. `hx`).
    #[serde(default = "default_helix")]
    pub helix: String,
    /// Terminal emulator to launch the helix editor in (empty = in-place).
    #[serde(default = "default_foot")]
    pub helix_terminal: String,
    /// `v` key: editor command (e.g. `nvim`).
    #[serde(default = "default_neovim")]
    pub neovim: String,
    /// Terminal emulator to launch the neovim editor in (empty = in-place).
    #[serde(default = "default_alacritty")]
    pub neovim_terminal: String,
}

impl Default for EditorsConfig {
    fn default() -> Self {
        Self {
            helix: default_helix(),
            helix_terminal: default_foot(),
            neovim: default_neovim(),
            neovim_terminal: default_alacritty(),
        }
    }
}

fn default_helix() -> String {
    "hx".to_string()
}

fn default_foot() -> String {
    "footclient".to_string()
}

fn default_neovim() -> String {
    "nvim".to_string()
}

fn default_alacritty() -> String {
    "alacritty".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for CompanionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            host: default_host(),
            port: default_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CppConfig {
    #[serde(default = "default_compiler")]
    pub compiler: String,
    #[serde(default = "default_std")]
    pub standard: String,
    #[serde(default = "default_flags")]
    pub flags: Vec<String>,
}

impl Default for CppConfig {
    fn default() -> Self {
        Self {
            compiler: default_compiler(),
            standard: default_std(),
            flags: default_flags(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    #[serde(default = "default_timeout")]
    pub default_timeout_ms: u64,
    /// Multiplier applied to the time limit to allow local overhead.
    #[serde(default = "default_overhead")]
    pub overhead_multiplier: f64,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: default_timeout(),
            overhead_multiplier: default_overhead(),
        }
    }
}

fn default_workspace() -> String {
    "~/cp".to_string()
}
fn default_editor() -> String {
    "hx".to_string()
}
fn default_true() -> bool {
    true
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    27121
}
fn default_compiler() -> String {
    "g++".to_string()
}
fn default_std() -> String {
    "c++20".to_string()
}
fn default_flags() -> Vec<String> {
    vec![
        "-O2".into(),
        "-Wall".into(),
        "-Wextra".into(),
        "-Wshadow".into(),
        "-DLOCAL".into(),
    ]
}
fn default_timeout() -> u64 {
    2000
}
fn default_overhead() -> f64 {
    1.0
}

impl Default for Config {
    fn default() -> Self {
        Config {
            workspace: default_workspace(),
            editor: EditorConfig::default(),
            editors: EditorsConfig::default(),
            companion: CompanionConfig::default(),
            cpp: CppConfig::default(),
            runner: RunnerConfig::default(),
        }
    }
}

/// XDG-aware path resolver for cptui.
pub struct Paths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Self::new()
    }
}

impl Paths {
    pub fn new() -> Self {
        let proj = directories::ProjectDirs::from("", "", "cptui")
            .expect("cannot determine project directories");
        Self {
            config_dir: proj.config_dir().to_path_buf(),
            cache_dir: proj.cache_dir().to_path_buf(),
            // `state_dir` is `Option<&Path>` on some platforms; fall back to data_dir.
            state_dir: proj
                .state_dir()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| proj.data_dir().to_path_buf()),
            data_dir: proj.data_dir().to_path_buf(),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn session_file(&self) -> PathBuf {
        self.state_dir.join("session.json")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.cache_dir.join("bin")
    }

    pub fn binary_path(&self, problem_id: &str) -> PathBuf {
        let safe = sanitize(problem_id);
        self.bin_dir().join(format!("{safe}_solution"))
    }
}

/// Load config from disk, creating a default if absent.
pub fn load(paths: &Paths) -> Result<Config> {
    let path = paths.config_file();
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
    Ok(cfg)
}

/// Save config to disk (used when generating defaults / doctor fix).
pub fn save(paths: &Paths, cfg: &Config) -> Result<()> {
    let path = paths.config_file();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let raw = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Expand a leading `~` to the home directory.
pub fn expand_tilde(p: &str) -> Result<PathBuf> {
    if let Some(rest) = p.strip_prefix("~/") {
        let home = directories::BaseDirs::new()
            .context("cannot determine home directory")?
            .home_dir()
            .to_path_buf();
        Ok(home.join(rest))
    } else if p == "~" {
        let home = directories::BaseDirs::new()
            .context("cannot determine home directory")?
            .home_dir()
            .to_path_buf();
        Ok(home)
    } else {
        Ok(PathBuf::from(p))
    }
}

/// Make a path component safe for filesystem use.
pub fn sanitize(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else if c == ' ' {
            out.push('-');
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches(|c: char| c == '.' || c == '-' || c == '_');
    if trimmed.is_empty() {
        "problem".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn workspace_path(cfg: &Config) -> Result<PathBuf> {
    expand_tilde(&cfg.workspace)
}

/// Check whether a command exists in PATH.
pub fn which(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        let p = Path::new(cmd);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        return None;
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(cmd);
            if candidate.is_file() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}
