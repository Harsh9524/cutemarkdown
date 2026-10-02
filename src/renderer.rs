//! GPU renderer fallback chain.
//!
//! Some Windows setups (odd drivers, VMs, RDP, software Vulkan) fail to create a wgpu device with
//! the default backend (DX12), and `eframe::run_native` returns an error before the first frame.
//! winit can't create a second event loop in the same process, so instead of retrying in-process
//! we re-launch the exe with the next backend:
//!
//! ```text
//! default (wgpu's choice, DX12 on Windows) → WGPU_BACKEND=vulkan → WGPU_BACKEND=gl → message box
//! ```
//!
//! `CUTEMARKDOWN_RENDERER_ATTEMPT` counts attempts so the chain can never loop. The backend that
//! worked is saved in settings (`"renderer": "vulkan"`) and tried first next time.

use serde::{Deserialize, Serialize};

/// Env var carrying the attempt number (0 = first launch) to re-launched processes.
pub const ATTEMPT_ENV: &str = "CUTEMARKDOWN_RENDERER_ATTEMPT";
/// Read by egui-wgpu (`wgpu::Backends::from_env`).
pub const WGPU_BACKEND_ENV: &str = "WGPU_BACKEND";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Renderer {
    /// wgpu's default choice (DX12 on Windows).
    #[default]
    Auto,
    Vulkan,
    Gl,
}

/// The default order, used when nothing is known about the machine.
const DEFAULT_CHAIN: [Renderer; 3] = [Renderer::Auto, Renderer::Vulkan, Renderer::Gl];

impl Renderer {
    /// The `WGPU_BACKEND` value that selects this renderer (`None` = leave wgpu's default).
    pub fn wgpu_backend(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Vulkan => Some("vulkan"),
            Self::Gl => Some("gl"),
        }
    }
}

/// Renderers to try, in order: the one that worked last time first, then the rest of the default
/// chain.
pub fn chain(preferred: Renderer) -> Vec<Renderer> {
    std::iter::once(preferred)
        .chain(DEFAULT_CHAIN.into_iter().filter(|&r| r != preferred))
        .collect()
}

/// The renderer for attempt `attempt` (0-based), or `None` once every backend has been tried.
pub fn for_attempt(preferred: Renderer, attempt: usize) -> Option<Renderer> {
    chain(preferred).get(attempt).copied()
}

/// This process's attempt number (0 unless we were re-launched by a failed attempt).
pub fn current_attempt() -> usize {
    std::env::var(ATTEMPT_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Renderer::*;

    #[test]
    fn default_chain_is_dx12_vulkan_gl() {
        assert_eq!(chain(Auto), vec![Auto, Vulkan, Gl]);
        assert_eq!(for_attempt(Auto, 0), Some(Auto));
        assert_eq!(for_attempt(Auto, 1), Some(Vulkan));
        assert_eq!(for_attempt(Auto, 2), Some(Gl));
        assert_eq!(for_attempt(Auto, 3), None);
    }

    #[test]
    fn remembered_backend_goes_first_and_nothing_repeats() {
        assert_eq!(chain(Vulkan), vec![Vulkan, Auto, Gl]);
        assert_eq!(chain(Gl), vec![Gl, Auto, Vulkan]);
        for p in [Auto, Vulkan, Gl] {
            let c = chain(p);
            assert_eq!(c.len(), 3);
            assert!(DEFAULT_CHAIN.iter().all(|r| c.contains(r)));
            assert_eq!(for_attempt(p, 3), None, "chain must terminate");
        }
    }

    #[test]
    fn backend_env_values() {
        assert_eq!(Auto.wgpu_backend(), None);
        assert_eq!(Vulkan.wgpu_backend(), Some("vulkan"));
        assert_eq!(Gl.wgpu_backend(), Some("gl"));
    }
}
