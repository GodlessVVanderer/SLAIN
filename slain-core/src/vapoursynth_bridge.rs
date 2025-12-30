//! VapourSynth runtime loader (stubbed for portability).
//!
//! This module attempts to load VapourSynth DLLs when available and exposes
//! a small status API for pipeline integration. Full API bindings are out of
//! scope for this minimal bridge and can be added behind the `vapoursynth`
//! feature flag.

use std::path::{Path, PathBuf};

use libloading::Library;

#[derive(Debug, Clone)]
pub enum VapourSynthStatus {
    Available { vs_path: PathBuf, vsscript_path: PathBuf },
    Unavailable { reason: String },
}

#[derive(Debug)]
pub struct VapourSynthRuntime {
    _vs_lib: Option<Library>,
    _vsscript_lib: Option<Library>,
    status: VapourSynthStatus,
}

impl VapourSynthRuntime {
    pub fn try_load(vs_path: Option<&Path>, vsscript_path: Option<&Path>) -> Self {
        #[cfg(windows)]
        {
            let vs_default = PathBuf::from("vapoursynth.dll");
            let vsscript_default = PathBuf::from("vsscript.dll");
            let vs_path = vs_path.unwrap_or(&vs_default);
            let vsscript_path = vsscript_path.unwrap_or(&vsscript_default);

            let vs_lib = unsafe { Library::new(vs_path).ok() };
            let vsscript_lib = unsafe { Library::new(vsscript_path).ok() };

            if let (Some(vs_lib), Some(vsscript_lib)) = (vs_lib, vsscript_lib) {
                return Self {
                    _vs_lib: Some(vs_lib),
                    _vsscript_lib: Some(vsscript_lib),
                    status: VapourSynthStatus::Available {
                        vs_path: vs_path.to_path_buf(),
                        vsscript_path: vsscript_path.to_path_buf(),
                    },
                };
            }

            return Self {
                _vs_lib: None,
                _vsscript_lib: None,
                status: VapourSynthStatus::Unavailable {
                    reason: "VapourSynth DLLs not found".to_string(),
                },
            };
        }

        #[cfg(not(windows))]
        {
            let _ = (vs_path, vsscript_path);
            Self {
                _vs_lib: None,
                _vsscript_lib: None,
                status: VapourSynthStatus::Unavailable {
                    reason: "VapourSynth loader not available on this platform".to_string(),
                },
            }
        }
    }

    pub fn status(&self) -> &VapourSynthStatus {
        &self.status
    }

    pub fn is_available(&self) -> bool {
        matches!(self.status, VapourSynthStatus::Available { .. })
    }
}

impl Default for VapourSynthRuntime {
    fn default() -> Self {
        Self::try_load(None, None)
    }
}
