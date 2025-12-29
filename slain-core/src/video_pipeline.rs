//! Unified video processing pipeline glue.

use crate::deinterlace::{DeinterlaceConfig, Deinterlacer};
use crate::filter_pipeline::{FilterChain, FilterChainSpec, Frame, PixelFormat};

pub struct VideoPipeline {
    chain: FilterChain,
    deinterlacer: Option<Deinterlacer>,
}

impl VideoPipeline {
    pub fn new(spec: FilterChainSpec, width: u32, height: u32, deinterlace: Option<DeinterlaceConfig>) -> Self {
        let chain = FilterChain::new(spec);
        let deinterlacer = deinterlace.map(|config| Deinterlacer::new(config, width, height));
        Self { chain, deinterlacer }
    }

    pub fn process_frame(&mut self, mut frame: Frame) -> Frame {
        if let Some(deinterlacer) = self.deinterlacer.as_mut() {
            if frame.format == PixelFormat::PlanarRgb {
                let (frames, _) = deinterlacer.process(&frame.data);
                if let Some(first) = frames.first() {
                    frame.data = first.clone();
                }
            }
        }

        self.chain.process_frame(frame)
    }

    pub fn chain(&self) -> &FilterChain {
        &self.chain
    }

    pub fn chain_mut(&mut self) -> &mut FilterChain {
        &mut self.chain
    }
}
