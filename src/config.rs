use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub paths: PathsConfig,
    pub upload: UploadConfig,
    #[serde(default)]
    pub postprocess: PostProcessConfig,

    #[serde(skip)]
    pub emulators: HashMap<String, EmulatorConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    pub webhook_secret_env: String,
    #[serde(default = "default_job_retention_hours")]
    pub job_retention_hours: u64,
}

fn default_job_retention_hours() -> u64 {
    48
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathsConfig {
    pub rom_root: PathBuf,
    pub job_root: PathBuf,
    pub repo_root: PathBuf,
    pub secret_root: PathBuf,

    #[serde(default = "default_screenshotter_root")]
    pub screenshotter_root: PathBuf,
}

fn default_screenshotter_root() -> PathBuf {
    PathBuf::from(".")
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadConfig {
    #[serde(default = "default_workers")]
    pub workers: u32,
    pub submitted_by: String,
    pub r2_env_file: PathBuf,

    #[serde(default)]
    pub no_push: bool,
    #[serde(default)]
    pub dry_run: bool,
}

fn default_workers() -> u32 {
    100
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostProcessConfig {
    #[serde(default = "default_true")]
    pub remove_single_color: bool,
    #[serde(default = "default_solid_tolerance")]
    pub solid_tolerance: u8,
    #[serde(default = "default_true")]
    pub dedupe: bool,
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        PostProcessConfig {
            remove_single_color: true,
            solid_tolerance: default_solid_tolerance(),
            dedupe: true,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_solid_tolerance() -> u8 {
    2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    SinglePng,

    MultiFrame,
}

impl OutputMode {
    fn parse(s: &str) -> OutputMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "multi-frame" | "multi_frame" | "multiframe" => OutputMode::MultiFrame,
            _ => OutputMode::SinglePng,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmulatorConfig {
    pub slug: String,

    pub dir: PathBuf,

    pub rom_subdir: String,

    pub emulator_repo: String,

    #[allow(dead_code)]
    pub emulator_default_branch: Option<String>,

    pub archive_slug: String,
    #[allow(dead_code)]
    pub requires_gpu: bool,
    pub gpu_mode: GpuMode,

    pub max_parallel_games: u32,
    pub max_parallel_stages: u32,
    pub output_mode: OutputMode,
    pub docker_mounts: Vec<String>,
    pub docker_env: Vec<(String, String)>,

    pub supported_direct: Vec<String>,

    pub supported_archives: Vec<String>,

    pub skip_submodules: Vec<String>,

    pub shallow_submodules: bool,

    pub per_game_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtKind {
    Direct,

    Archive,

    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuMode {
    None,
    Nvidia,
    Dri,
}

impl GpuMode {
    fn parse(s: &str) -> GpuMode {
        match s.trim().to_ascii_lowercase().as_str() {
            "nvidia" | "all" | "gpus" => GpuMode::Nvidia,
            "dri" | "device" | "amd" | "intel" => GpuMode::Dri,
            _ => GpuMode::None,
        }
    }
}

impl EmulatorConfig {
    pub fn rom_base(&self, rom_root: &Path) -> PathBuf {
        if self.rom_subdir.is_empty() {
            rom_root.to_path_buf()
        } else {
            rom_root.join(&self.rom_subdir)
        }
    }

    pub fn classify_ext(&self, ext: &str) -> ExtKind {
        let ext = ext.to_ascii_lowercase();
        if self
            .supported_direct
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&ext))
        {
            ExtKind::Direct
        } else if self
            .supported_archives
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&ext))
        {
            ExtKind::Archive
        } else {
            ExtKind::Unsupported
        }
    }
}

#[derive(Debug, Deserialize)]
struct ShotterFile {
    slug: String,
    emulator_repo: String,
    #[serde(default)]
    emulator_default_branch: Option<String>,
    #[serde(default)]
    rom_subdir: Option<String>,
    archive_slug: String,
    #[serde(default)]
    supported_direct: Vec<String>,
    #[serde(default)]
    supported_archives: Vec<String>,
    #[serde(default)]
    skip_submodules: Vec<String>,
    #[serde(default)]
    runner: ShotterRunner,
}

#[derive(Debug, Default, Deserialize)]
struct ShotterRunner {
    #[serde(default)]
    requires_gpu: bool,
    #[serde(default)]
    gpu: Option<String>,
    #[serde(default = "default_parallel")]
    max_parallel_games: u32,
    #[serde(default = "default_parallel_stages")]
    max_parallel_stages: u32,
    #[serde(default)]
    output_mode: Option<String>,
    #[serde(default)]
    docker_mounts: Vec<String>,
    #[serde(default)]
    docker_env: std::collections::HashMap<String, String>,
    #[serde(default)]
    per_game_args: Vec<String>,
    #[serde(default)]
    shallow_submodules: bool,
}

fn default_parallel() -> u32 {
    1
}

fn default_parallel_stages() -> u32 {
    4
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config file {}", path.display()))?;
        cfg.emulators = discover_emulators(&cfg.paths.screenshotter_root)?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.emulators.is_empty() {
            return Err(anyhow!(
                "no screenshotters found under {} (expected subdirs containing screenshotter.toml)",
                self.paths.screenshotter_root.display()
            ));
        }
        Ok(())
    }

    pub fn webhook_secret(&self) -> Result<String> {
        std::env::var(&self.server.webhook_secret_env).map_err(|_| {
            anyhow!(
                "webhook secret env var '{}' is not set",
                self.server.webhook_secret_env
            )
        })
    }

    pub fn emulator(&self, slug: &str) -> Option<&EmulatorConfig> {
        self.emulators.get(slug)
    }
}

fn discover_emulators(root: &Path) -> Result<HashMap<String, EmulatorConfig>> {
    let mut map = HashMap::new();
    let entries = std::fs::read_dir(root)
        .with_context(|| format!("scanning screenshotter root {}", root.display()))?;

    for entry in entries {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let toml_path = dir.join("screenshotter.toml");
        if !toml_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&toml_path)
            .with_context(|| format!("reading {}", toml_path.display()))?;
        let sf: ShotterFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", toml_path.display()))?;

        let emu = EmulatorConfig {
            slug: sf.slug.clone(),
            dir,
            rom_subdir: sf.rom_subdir.unwrap_or_default(),
            emulator_repo: sf.emulator_repo,
            emulator_default_branch: sf.emulator_default_branch,
            archive_slug: sf.archive_slug,
            requires_gpu: sf.runner.requires_gpu,
            gpu_mode: sf.runner.gpu.as_deref().map(GpuMode::parse).unwrap_or(
                if sf.runner.requires_gpu {
                    GpuMode::Nvidia
                } else {
                    GpuMode::None
                },
            ),
            max_parallel_games: sf.runner.max_parallel_games,
            max_parallel_stages: sf.runner.max_parallel_stages,
            output_mode: sf
                .runner
                .output_mode
                .as_deref()
                .map(OutputMode::parse)
                .unwrap_or(OutputMode::SinglePng),
            docker_mounts: sf.runner.docker_mounts,
            docker_env: {
                let mut v: Vec<(String, String)> = sf.runner.docker_env.into_iter().collect();
                v.sort();
                v
            },
            supported_direct: sf.supported_direct,
            supported_archives: sf.supported_archives,
            skip_submodules: sf.skip_submodules,
            shallow_submodules: sf.runner.shallow_submodules,
            per_game_args: sf.runner.per_game_args,
        };

        if let Some(prev) = map.insert(sf.slug.clone(), emu) {
            return Err(anyhow!(
                "duplicate screenshotter slug '{}' (also defined at {})",
                sf.slug,
                prev.dir.display()
            ));
        }
    }
    Ok(map)
}
