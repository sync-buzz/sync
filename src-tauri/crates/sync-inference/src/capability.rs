//! What this machine can run a model on, read at compile time.
//!
//! The catalogue filters models against this so a person is not offered
//! something their hardware cannot load. The filter refuses with a sentence
//! rather than hiding the row — a model that needs Apple Silicon is shown
//! disabled with the reason, so a person may override, and a person who
//! expected to see one learns why they do not.
//!
//! Everything here is known to the compiler, which is the lazy first rung: arch
//! and OS and Metal-availability are `cfg` facts, and reading them at runtime
//! would be a second answer to a question the build already answers. Free
//! memory is not, and the memory check is left to the caller (it needs a live
//! reading the catalogue does not have at build time).

use crate::{ModelManifest, Runtime};

/// What this build can run a model on.
///
/// Built from `cfg` at compile time, so it is a constant for a given binary
/// and costs nothing to read. The catalogue calls [`Capabilities::here`] and
/// holds the one answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// The CPU architecture, as `gguf`/`mlx`/`onnx` runtimes name it.
    pub arch: &'static str,
    /// `macos`, `linux` or `windows`. `mlx` is macOS-only.
    pub os: &'static str,
    /// `metal` on Apple Silicon, `cuda` when detected, otherwise `none`.
    pub gpu: &'static str,
}

impl Capabilities {
    /// This build's capabilities. A constant for a given binary.
    #[must_use]
    pub const fn here() -> Self {
        Self {
            arch: arch(),
            os: os(),
            gpu: gpu(),
        }
    }
}

/// The architecture as a runtime names it.
const fn arch() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "aarch64"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        "unknown"
    }
}

/// The operating system, lower-case.
const fn os() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unknown"
    }
}

/// The GPU the build links for. Metal on Apple Silicon; CUDA left to the
/// runtime's own detection, so `none` here is not a claim that no GPU exists,
/// only that this build did not link one.
const fn gpu() -> &'static str {
    // Metal is the Apple-Silicon path and is implied by `aarch64` + `macos`.
    // Intel macs have no Metal-accelerated llama.cpp path that this build
    // takes, so they read as `none` and the filter falls back to CPU models.
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "metal"
    }
    #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
    {
        "none"
    }
}

/// Why a model cannot run here, in a sentence, or `None` when it can.
///
/// The refusal is shown beside the row rather than hiding it, so a person may
/// override and a person who expected to see one learns why they do not. The
/// sentence is written for somebody deciding what to do next, not for somebody
/// diagnosing the build: it names what the model needs and what this machine
/// has, and nothing about a protocol or a version.
#[must_use]
pub fn refusal_of(manifest: &ModelManifest, caps: &Capabilities) -> Option<String> {
    // MLX is Apple-silicon only. A model declaring it anywhere else is a model
    // the build has no runtime for, and the honest refusal names that rather
    // than a missing GPU.
    if manifest.runtime == Runtime::Mlx && !(caps.os == "macos" && caps.arch == "aarch64") {
        return Some(format!(
            "{} needs MLX, which runs on Apple Silicon. This machine is {} {}.",
            manifest.name, caps.arch, caps.os
        ));
    }

    // The GPU requirement, read against what this build links. `any` and
    // `none` are both satisfied by a CPU-only build: `none` is a model that
    // asks for no GPU, and `any` is one that runs with or without.
    let gpu = manifest.requires.gpu.as_str();
    if (gpu == "metal" && caps.gpu != "metal") || (gpu == "cuda" && caps.gpu != "cuda") {
        return Some(format!(
            "{} needs a {} GPU, and this build has {}.",
            manifest.name, gpu, caps.gpu
        ));
    }

    // The architecture list, when the model names one. Empty means any.
    if !manifest.requires.arch.is_empty() && !manifest.requires.arch.iter().any(|a| a == caps.arch)
    {
        return Some(format!(
            "{} runs on {} and this machine is {}.",
            manifest.name,
            manifest.requires.arch.join(" or "),
            caps.arch
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Modality, Requires, Runtime, Weights};

    fn a_model(runtime: Runtime, requires: Requires) -> ModelManifest {
        ModelManifest {
            id: "x".to_owned(),
            version: "0".to_owned(),
            name: "X".to_owned(),
            modality: Modality::Text,
            runtime,
            purposes: vec!["text.classify".to_owned()],
            category: None,
            size: 1,
            min_memory: 0,
            requires,
            weights: Weights {
                url: "u".to_owned(),
                sha256: "h".to_owned(),
                format: None,
            },
            engine: None,
            source: None,
            license: None,
            schemas: std::collections::BTreeMap::new(),
        }
    }

    /// The capabilities this build reports are internally consistent: Metal is
    /// reported only on Apple Silicon, which is the one machine that has it.
    #[test]
    fn the_builds_capabilities_are_consistent() {
        let caps = Capabilities::here();
        if caps.gpu == "metal" {
            assert_eq!(caps.arch, "aarch64");
            assert_eq!(caps.os, "macos");
        }
    }

    /// An MLX model off Apple Silicon is refused, and the sentence names MLX
    /// rather than a missing GPU.
    #[test]
    fn an_mlx_model_off_apple_silicon_is_refused() {
        let caps = Capabilities {
            arch: "x86_64",
            os: "linux",
            gpu: "none",
        };
        let model = a_model(Runtime::Mlx, Requires::default());
        let refusal = refusal_of(&model, &caps).expect("MLX off Apple Silicon is refused");
        assert!(refusal.contains("MLX"), "{refusal}");
    }

    /// A model that asks for `any` GPU runs anywhere, which is what a CPU model
    /// declares.
    #[test]
    fn an_any_gpu_model_is_not_refused() {
        let caps = Capabilities {
            arch: "x86_64",
            os: "linux",
            gpu: "none",
        };
        let model = a_model(Runtime::Gguf, Requires::default());
        assert!(refusal_of(&model, &caps).is_none());
    }

    /// A model that asks for Metal is refused where the build has none.
    #[test]
    fn a_metal_model_on_a_cpu_build_is_refused() {
        let caps = Capabilities {
            arch: "x86_64",
            os: "linux",
            gpu: "none",
        };
        let model = a_model(
            Runtime::Gguf,
            Requires {
                arch: vec![],
                gpu: "metal".to_owned(),
            },
        );
        let refusal = refusal_of(&model, &caps).expect("a metal model is refused on a cpu build");
        assert!(refusal.contains("metal"), "{refusal}");
    }
}
