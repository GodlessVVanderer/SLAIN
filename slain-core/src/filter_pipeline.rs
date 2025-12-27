//! # Filter Pipeline
//!
//! Global filter registry and per-playback filter chains.

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::pipeline::{PipelineConfig, PipelineKind};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContainerFormat {
    Mp4,
    Mkv,
    Avi,
    Ts,
    Mov,
    Webm,
    Other(String),
}

impl ContainerFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "mp4" | "m4v" => Some(ContainerFormat::Mp4),
            "mkv" => Some(ContainerFormat::Mkv),
            "avi" => Some(ContainerFormat::Avi),
            "ts" | "m2ts" => Some(ContainerFormat::Ts),
            "mov" => Some(ContainerFormat::Mov),
            "webm" => Some(ContainerFormat::Webm),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    pub fn label(&self) -> &str {
        match self {
            ContainerFormat::Mp4 => "MP4",
            ContainerFormat::Mkv => "MKV",
            ContainerFormat::Avi => "AVI",
            ContainerFormat::Ts => "TS",
            ContainerFormat::Mov => "MOV",
            ContainerFormat::Webm => "WebM",
            ContainerFormat::Other(label) => label.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CodecKind {
    H264,
    H265,
    Vp9,
    Av1,
    Mpeg2,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Nv12,
    I420,
    Rgba,
    Bgra,
    PlanarRgb,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct FilterCapabilities {
    pub codecs: Vec<CodecKind>,
    pub containers: Vec<ContainerFormat>,
    pub pixel_formats: Vec<PixelFormat>,
}

impl FilterCapabilities {
    pub fn any() -> Self {
        Self {
            codecs: Vec::new(),
            containers: Vec::new(),
            pixel_formats: Vec::new(),
        }
    }

    pub fn supports_container(&self, container: &ContainerFormat) -> bool {
        self.containers.is_empty() || self.containers.contains(container)
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub pts_us: i64,
}

pub trait Filter: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> FilterCapabilities;
    fn process_frame(&mut self, frame: Frame) -> Frame;
}

#[derive(Debug, Clone)]
pub struct FilterDescriptor {
    pub name: String,
    pub priority: i32,
    pub capabilities: FilterCapabilities,
}

type FilterFactory = dyn Fn() -> Box<dyn Filter> + Send + Sync;

#[derive(Clone)]
struct RegisteredFilter {
    descriptor: FilterDescriptor,
    factory: std::sync::Arc<FilterFactory>,
}

#[derive(Debug, Clone)]
pub struct FilterChainSpec {
    pub container: Option<ContainerFormat>,
    pub filters: Vec<String>,
}

impl FilterChainSpec {
    pub fn empty(container: Option<ContainerFormat>) -> Self {
        Self {
            container,
            filters: Vec::new(),
        }
    }

    pub fn display_chain(&self) -> String {
        if self.filters.is_empty() {
            "(disabled)".to_string()
        } else {
            self.filters.join(" → ")
        }
    }
}

pub struct FilterChain {
    filters: Vec<Box<dyn Filter>>,
    spec: FilterChainSpec,
}

impl FilterChain {
    pub fn new(spec: FilterChainSpec) -> Self {
        Self {
            filters: Vec::new(),
            spec,
        }
    }

    pub fn add_filter(&mut self, filter: Box<dyn Filter>) {
        self.filters.push(filter);
    }

    pub fn process_frame(&mut self, mut frame: Frame) -> Frame {
        for filter in &mut self.filters {
            frame = filter.process_frame(frame);
        }
        frame
    }

    pub fn spec(&self) -> &FilterChainSpec {
        &self.spec
    }

    pub fn filter_names(&self) -> Vec<String> {
        self.filters.iter().map(|f| f.name().to_string()).collect()
    }
}

pub struct FilterRegistry {
    filters: Vec<RegisteredFilter>,
    default_chains: HashMap<ContainerFormat, FilterChainSpec>,
    user_overrides: HashMap<ContainerFormat, FilterChainSpec>,
}

impl FilterRegistry {
    pub fn global() -> &'static RwLock<Self> {
        static REGISTRY: Lazy<RwLock<FilterRegistry>> =
            Lazy::new(|| RwLock::new(FilterRegistry::with_defaults()));
        &REGISTRY
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self {
            filters: Vec::new(),
            default_chains: HashMap::new(),
            user_overrides: HashMap::new(),
        };

        registry.register_builtin_filters();
        registry.register_default_chains();
        registry
    }

    pub fn register_filter(
        &mut self,
        name: impl Into<String>,
        priority: i32,
        capabilities: FilterCapabilities,
        factory: impl Fn() -> Box<dyn Filter> + Send + Sync + 'static,
    ) {
        let descriptor = FilterDescriptor {
            name: name.into(),
            priority,
            capabilities,
        };
        self.filters.push(RegisteredFilter {
            descriptor,
            factory: std::sync::Arc::new(factory),
        });
        self.filters
            .sort_by(|a, b| b.descriptor.priority.cmp(&a.descriptor.priority));
    }

    pub fn list_filters(&self) -> Vec<FilterDescriptor> {
        self.filters.iter().map(|f| f.descriptor.clone()).collect()
    }

    pub fn chain_spec_for(&self, container: &ContainerFormat) -> FilterChainSpec {
        self.user_overrides
            .get(container)
            .cloned()
            .or_else(|| self.default_chains.get(container).cloned())
            .unwrap_or_else(|| FilterChainSpec::empty(Some(container.clone())))
    }

    pub fn default_chain_spec(&self, container: &ContainerFormat) -> Option<&FilterChainSpec> {
        self.default_chains.get(container)
    }

    pub fn user_override_spec(&self, container: &ContainerFormat) -> Option<&FilterChainSpec> {
        self.user_overrides.get(container)
    }

    pub fn set_user_override(&mut self, container: ContainerFormat, spec: FilterChainSpec) {
        self.user_overrides.insert(container, spec);
    }

    pub fn clear_user_override(&mut self, container: &ContainerFormat) {
        self.user_overrides.remove(container);
    }

    pub fn build_chain(&self, spec: FilterChainSpec) -> FilterChain {
        let mut chain = FilterChain::new(spec.clone());
        for name in &spec.filters {
            if let Some(entry) = self.filters.iter().find(|f| &f.descriptor.name == name) {
                let filter = (entry.factory)();
                chain.add_filter(filter);
            }
        }
        chain
    }

    fn register_builtin_filters(&mut self) {
        self.register_filter("Color Correct", 80, FilterCapabilities::any(), || {
            Box::new(BasicFilter::new("Color Correct"))
        });
        self.register_filter("Denoise", 70, FilterCapabilities::any(), || {
            Box::new(BasicFilter::new("Denoise"))
        });
        self.register_filter("Sharpen", 60, FilterCapabilities::any(), || {
            Box::new(BasicFilter::new("Sharpen"))
        });
        self.register_filter("Deband", 50, FilterCapabilities::any(), || {
            Box::new(BasicFilter::new("Deband"))
        });
        self.register_filter("Deinterlace", 40, FilterCapabilities::any(), || {
            Box::new(BasicFilter::new("Deinterlace"))
        });
    }

    fn register_default_chains(&mut self) {
        self.default_chains.insert(
            ContainerFormat::Mp4,
            FilterChainSpec {
                container: Some(ContainerFormat::Mp4),
                filters: vec!["Color Correct".into(), "Sharpen".into()],
            },
        );
        self.default_chains.insert(
            ContainerFormat::Mkv,
            FilterChainSpec {
                container: Some(ContainerFormat::Mkv),
                filters: vec!["Denoise".into(), "Deband".into()],
            },
        );
        self.default_chains.insert(
            ContainerFormat::Avi,
            FilterChainSpec {
                container: Some(ContainerFormat::Avi),
                filters: vec!["Deinterlace".into(), "Denoise".into()],
            },
        );
        self.default_chains.insert(
            ContainerFormat::Ts,
            FilterChainSpec {
                container: Some(ContainerFormat::Ts),
                filters: vec!["Deinterlace".into(), "Color Correct".into()],
            },
        );
    }
}

#[derive(Debug, Clone)]
pub struct PipelineProfile {
    pub name: String,
    pub pipeline_kind: PipelineKind,
    pub config: Option<PipelineConfig>,
    pub filter_chain: FilterChainSpec,
}

impl PipelineProfile {
    pub fn new(
        name: impl Into<String>,
        pipeline_kind: PipelineKind,
        config: Option<PipelineConfig>,
        filter_chain: FilterChainSpec,
    ) -> Self {
        Self {
            name: name.into(),
            pipeline_kind,
            config,
            filter_chain,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PipelineProfileSelector {
    global: PipelineProfile,
    per_file: HashMap<PathBuf, PipelineProfile>,
}

impl PipelineProfileSelector {
    pub fn new(global: PipelineProfile) -> Self {
        Self {
            global,
            per_file: HashMap::new(),
        }
    }

    pub fn global(&self) -> &PipelineProfile {
        &self.global
    }

    pub fn set_global(&mut self, profile: PipelineProfile) {
        self.global = profile;
    }

    pub fn set_for_file(&mut self, path: PathBuf, profile: PipelineProfile) {
        self.per_file.insert(path, profile);
    }

    pub fn clear_for_file(&mut self, path: &Path) {
        self.per_file.remove(path);
    }

    pub fn profile_for(&self, path: Option<&Path>) -> &PipelineProfile {
        if let Some(path) = path {
            if let Some(profile) = self.per_file.get(path) {
                return profile;
            }
        }
        &self.global
    }

    pub fn scope_for(&self, path: Option<&Path>) -> ProfileScope {
        if let Some(path) = path {
            if self.per_file.contains_key(path) {
                return ProfileScope::PerFile;
            }
        }
        ProfileScope::Global
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileScope {
    Global,
    PerFile,
}

#[derive(Debug, Clone, Copy)]
pub enum ScriptEngineKind {
    AviSynth,
    VapourSynth,
}

pub struct ScriptFilterCompiler {
    engine: ScriptEngineKind,
}

impl ScriptFilterCompiler {
    pub fn new(engine: ScriptEngineKind) -> Self {
        Self { engine }
    }

    pub fn compile(&self, script: &str) -> FilterChain {
        let spec = FilterChainSpec {
            container: None,
            filters: vec![format!("{} Script", self.engine_label())],
        };
        let mut chain = FilterChain::new(spec.clone());
        chain.add_filter(Box::new(ScriptFilter::new(self.engine, script)));
        chain
    }

    fn engine_label(&self) -> &'static str {
        match self.engine {
            ScriptEngineKind::AviSynth => "AviSynth",
            ScriptEngineKind::VapourSynth => "VapourSynth",
        }
    }
}

struct BasicFilter {
    name: String,
}

impl BasicFilter {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Filter for BasicFilter {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> FilterCapabilities {
        FilterCapabilities::any()
    }

    fn process_frame(&mut self, frame: Frame) -> Frame {
        frame
    }
}

struct ScriptFilter {
    engine: ScriptEngineKind,
    script: String,
}

impl ScriptFilter {
    fn new(engine: ScriptEngineKind, script: &str) -> Self {
        Self {
            engine,
            script: script.to_string(),
        }
    }
}

impl Filter for ScriptFilter {
    fn name(&self) -> &str {
        match self.engine {
            ScriptEngineKind::AviSynth => "AviSynth Script",
            ScriptEngineKind::VapourSynth => "VapourSynth Script",
        }
    }

    fn capabilities(&self) -> FilterCapabilities {
        FilterCapabilities::any()
    }

    fn process_frame(&mut self, frame: Frame) -> Frame {
        let _ = &self.script;
        frame
    }
}
