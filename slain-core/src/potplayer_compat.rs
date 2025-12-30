//! PotPlayer-style filter chain compatibility helpers.

use crate::filter_pipeline::{ContainerFormat, FilterChainSpec};

#[derive(Debug, Clone, Copy)]
pub struct PotPlayerCompatConfig {
    pub enable_deinterlace: bool,
    pub enable_interpolation: bool,
}

impl Default for PotPlayerCompatConfig {
    fn default() -> Self {
        Self {
            enable_deinterlace: true,
            enable_interpolation: false,
        }
    }
}

impl PotPlayerCompatConfig {
    pub fn to_filter_chain(self, container: Option<ContainerFormat>) -> FilterChainSpec {
        let mut filters = Vec::new();
        if self.enable_deinterlace {
            filters.push("Deinterlace".to_string());
        }
        if self.enable_interpolation {
            filters.push("Motion Interpolation".to_string());
        }
        FilterChainSpec { container, filters }
    }
}
