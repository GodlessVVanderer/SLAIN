//! CPU deinterlacing utilities for RGB frames.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeinterlaceMode {
    Off,
    Auto,
    Bob,
    Weave,
    Blend,
    Yadif,
    Bwdif,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldOrder {
    TopFirst,
    BottomFirst,
    Auto,
}

#[derive(Debug, Clone, Copy)]
pub struct DeinterlaceConfig {
    pub mode: DeinterlaceMode,
    pub field_order: FieldOrder,
    pub force: bool,
    pub double_rate: bool,
}

impl Default for DeinterlaceConfig {
    fn default() -> Self {
        Self {
            mode: DeinterlaceMode::Auto,
            field_order: FieldOrder::Auto,
            force: false,
            double_rate: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InterlaceInfo {
    pub is_interlaced: bool,
    pub field_order: FieldOrder,
    pub confidence: f32,
    pub comb_factor: f32,
}

pub struct InterlaceDetector {
    avg_comb: f32,
    frame_count: u32,
    detected_order: FieldOrder,
}

impl InterlaceDetector {
    pub fn new() -> Self {
        Self {
            avg_comb: 0.0,
            frame_count: 0,
            detected_order: FieldOrder::Auto,
        }
    }

    pub fn analyze(&mut self, frame: &[u8], width: u32, height: u32) -> InterlaceInfo {
        let stride = width as usize * 3;
        let comb_factor = self.calculate_comb_factor(frame, width, height, stride);

        self.avg_comb = (self.avg_comb * self.frame_count as f32 + comb_factor)
            / (self.frame_count + 1) as f32;
        self.frame_count += 1;

        if self.frame_count < 10 && self.detected_order == FieldOrder::Auto {
            self.detected_order = self.detect_field_order(frame, width, height, stride);
        }

        let is_interlaced = comb_factor > 15.0;
        let confidence = (comb_factor / 50.0).clamp(0.0, 1.0);

        InterlaceInfo {
            is_interlaced,
            field_order: self.detected_order,
            confidence,
            comb_factor,
        }
    }

    fn calculate_comb_factor(
        &self,
        frame: &[u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> f32 {
        let mut comb_sum = 0i64;
        let mut pixel_count = 0u64;

        for y in 1..(height - 1) as usize {
            for x in (0..width as usize).step_by(4) {
                let idx = y * stride + x * 3;
                if idx + stride >= frame.len() || idx < stride {
                    continue;
                }
                let above = luma_at(frame, idx - stride);
                let current = luma_at(frame, idx);
                let below = luma_at(frame, idx + stride);

                let comb = (2 * current - above - below).abs();
                comb_sum += comb as i64;
                pixel_count += 1;
            }
        }

        if pixel_count > 0 {
            comb_sum as f32 / pixel_count as f32
        } else {
            0.0
        }
    }

    fn detect_field_order(
        &self,
        frame: &[u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> FieldOrder {
        let mut top_motion = 0i64;
        let mut bottom_motion = 0i64;

        for y in 2..(height - 2) as usize {
            for x in (0..width as usize).step_by(8) {
                let idx = y * stride + x * 3;
                if idx + 2 * stride >= frame.len() || idx < 2 * stride {
                    continue;
                }

                let current = luma_at(frame, idx);
                let two_up = luma_at(frame, idx - 2 * stride);
                let two_down = luma_at(frame, idx + 2 * stride);

                let diff = (2 * current - two_up - two_down).abs() as i64;

                if y % 2 == 0 {
                    top_motion += diff;
                } else {
                    bottom_motion += diff;
                }
            }
        }

        if top_motion > bottom_motion * 110 / 100 {
            FieldOrder::TopFirst
        } else if bottom_motion > top_motion * 110 / 100 {
            FieldOrder::BottomFirst
        } else {
            FieldOrder::TopFirst
        }
    }

    pub fn reset(&mut self) {
        self.avg_comb = 0.0;
        self.frame_count = 0;
        self.detected_order = FieldOrder::Auto;
    }
}

pub struct Deinterlacer {
    config: DeinterlaceConfig,
    detector: InterlaceDetector,
    prev_frame: Option<Vec<u8>>,
    width: u32,
    height: u32,
}

impl Deinterlacer {
    pub fn new(config: DeinterlaceConfig, width: u32, height: u32) -> Self {
        Self {
            config,
            detector: InterlaceDetector::new(),
            prev_frame: None,
            width,
            height,
        }
    }

    pub fn process(&mut self, frame: &[u8]) -> (Vec<Vec<u8>>, bool) {
        let info = self.detector.analyze(frame, self.width, self.height);

        let should_deinterlace = match self.config.mode {
            DeinterlaceMode::Off => false,
            DeinterlaceMode::Auto => info.is_interlaced || self.config.force,
            _ => true,
        };

        if !should_deinterlace {
            self.prev_frame = Some(frame.to_vec());
            return (vec![frame.to_vec()], false);
        }

        let field_order = match self.config.field_order {
            FieldOrder::Auto => info.field_order,
            other => other,
        };

        let output = match self.config.mode {
            DeinterlaceMode::Off => vec![frame.to_vec()],
            DeinterlaceMode::Auto | DeinterlaceMode::Yadif => self.yadif(frame, field_order),
            DeinterlaceMode::Bob => self.bob(frame, field_order),
            DeinterlaceMode::Weave => self.weave(frame, field_order),
            DeinterlaceMode::Blend => self.blend(frame),
            DeinterlaceMode::Bwdif => self.yadif(frame, field_order),
        };

        self.prev_frame = Some(frame.to_vec());
        (output, true)
    }

    fn bob(&self, frame: &[u8], field_order: FieldOrder) -> Vec<Vec<u8>> {
        let height = self.height as usize;
        let stride = self.width as usize * 3;
        let output_size = stride * height;

        let first_field = match field_order {
            FieldOrder::TopFirst | FieldOrder::Auto => 0,
            FieldOrder::BottomFirst => 1,
        };

        let mut outputs = Vec::new();

        for field in 0..2 {
            let current_field = (first_field + field) % 2;
            let mut output = vec![0u8; output_size];

            for y in 0..height {
                if y % 2 != current_field {
                    let above = if y > 0 { y - 1 } else { 1 };
                    let below = if y < height - 1 { y + 1 } else { height - 2 };

                    for x in 0..stride {
                        let a = frame[above * stride + x] as u16;
                        let b = frame[below * stride + x] as u16;
                        output[y * stride + x] = ((a + b) / 2) as u8;
                    }
                } else {
                    output[y * stride..(y + 1) * stride]
                        .copy_from_slice(&frame[y * stride..(y + 1) * stride]);
                }
            }

            outputs.push(output);

            if !self.config.double_rate {
                break;
            }
        }

        outputs
    }

    fn weave(&self, frame: &[u8], field_order: FieldOrder) -> Vec<Vec<u8>> {
        let height = self.height as usize;
        let stride = self.width as usize * 3;

        let prev = match &self.prev_frame {
            Some(p) => p,
            None => return vec![frame.to_vec()],
        };

        let mut output = vec![0u8; stride * height];

        let (even_src, odd_src) = match field_order {
            FieldOrder::TopFirst | FieldOrder::Auto => (frame, prev.as_slice()),
            FieldOrder::BottomFirst => (prev.as_slice(), frame),
        };

        for y in 0..height {
            let src = if y % 2 == 0 { even_src } else { odd_src };
            output[y * stride..(y + 1) * stride]
                .copy_from_slice(&src[y * stride..(y + 1) * stride]);
        }

        vec![output]
    }

    fn blend(&self, frame: &[u8]) -> Vec<Vec<u8>> {
        let height = self.height as usize;
        let stride = self.width as usize * 3;

        let mut output = vec![0u8; stride * height];

        output[..stride].copy_from_slice(&frame[..stride]);

        for y in 1..(height - 1) {
            for x in 0..stride {
                let above = frame[(y - 1) * stride + x] as u16;
                let current = frame[y * stride + x] as u16;
                let below = frame[(y + 1) * stride + x] as u16;
                output[y * stride + x] = ((above + 2 * current + below) / 4) as u8;
            }
        }

        output[(height - 1) * stride..].copy_from_slice(&frame[(height - 1) * stride..]);

        vec![output]
    }

    fn yadif(&self, frame: &[u8], field_order: FieldOrder) -> Vec<Vec<u8>> {
        let height = self.height as usize;
        let stride = self.width as usize * 3;

        let prev = self.prev_frame.as_ref().map(|p| p.as_slice()).unwrap_or(frame);

        let first_field = match field_order {
            FieldOrder::TopFirst | FieldOrder::Auto => 0,
            FieldOrder::BottomFirst => 1,
        };

        let mut outputs = Vec::new();
        let passes = if self.config.double_rate { 2 } else { 1 };

        for field in 0..passes {
            let current_field = (first_field + field) % 2;
            let mut output = vec![0u8; stride * height];

            for y in 0..height {
                if y % 2 == current_field {
                    output[y * stride..(y + 1) * stride]
                        .copy_from_slice(&frame[y * stride..(y + 1) * stride]);
                } else {
                    for x in 0..stride {
                        let idx = y * stride + x;
                        let c = frame[idx] as i32;
                        let d = if y > 0 { frame[(y - 1) * stride + x] as i32 } else { c };
                        let e = if y < height - 1 {
                            frame[(y + 1) * stride + x] as i32
                        } else {
                            c
                        };

                        let p_c = prev[idx] as i32;
                        let p_d = if y > 0 { prev[(y - 1) * stride + x] as i32 } else { p_c };
                        let p_e = if y < height - 1 {
                            prev[(y + 1) * stride + x] as i32
                        } else {
                            p_c
                        };

                        let spatial = (d + e) / 2;
                        let temporal = (p_d + p_e) / 2;

                        let edge_h = (d - e).abs();
                        let edge_t = ((d - p_d).abs() + (e - p_e).abs()) / 2;

                        let result = if edge_t < edge_h { temporal } else { spatial };
                        output[idx] = result.clamp(0, 255) as u8;
                    }
                }
            }

            outputs.push(output);
        }

        outputs
    }

    pub fn reset(&mut self) {
        self.prev_frame = None;
        self.detector.reset();
    }

    pub fn config(&self) -> &DeinterlaceConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: DeinterlaceConfig) {
        self.config = config;
    }
}

fn luma_at(frame: &[u8], idx: usize) -> i32 {
    if idx + 2 >= frame.len() {
        return 0;
    }
    let r = frame[idx] as f32;
    let g = frame[idx + 1] as f32;
    let b = frame[idx + 2] as f32;
    (0.299 * r + 0.587 * g + 0.114 * b).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_interlaced_pattern() {
        let width = 64u32;
        let height = 64u32;
        let stride = width as usize * 3;
        let mut frame = vec![0u8; stride * height as usize];

        for y in 0..height as usize {
            for x in 0..width as usize {
                let value = if y % 2 == 0 { 200 } else { 50 };
                let idx = y * stride + x * 3;
                frame[idx] = value;
                frame[idx + 1] = value;
                frame[idx + 2] = value;
            }
        }

        let mut detector = InterlaceDetector::new();
        let info = detector.analyze(&frame, width, height);
        assert!(info.is_interlaced);
        assert!(info.comb_factor > 10.0);
    }

    #[test]
    fn bob_outputs_two_frames_when_double_rate() {
        let config = DeinterlaceConfig {
            mode: DeinterlaceMode::Bob,
            double_rate: true,
            ..Default::default()
        };
        let mut deinterlacer = Deinterlacer::new(config, 8, 8);
        let frame = vec![128u8; 8 * 8 * 3];
        let (outputs, deinterlaced) = deinterlacer.process(&frame);
        assert!(deinterlaced);
        assert_eq!(outputs.len(), 2);
    }
}
