//! Simple CPU frame interpolation utilities.
//!
//! This provides a reference implementation for blending RGB24 frames.

#[derive(Debug, Clone)]
pub struct RgbFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbFrame {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self, String> {
        let expected = width as usize * height as usize * 3;
        if data.len() != expected {
            return Err(format!(
                "RGB frame size mismatch: expected {} bytes, got {}",
                expected,
                data.len()
            ));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }
}

pub fn interpolate_rgb(prev: &RgbFrame, next: &RgbFrame, alpha: f32) -> Result<RgbFrame, String> {
    if prev.width != next.width || prev.height != next.height {
        return Err("Frame dimensions must match for interpolation".to_string());
    }
    if prev.data.len() != next.data.len() {
        return Err("Frame data sizes must match for interpolation".to_string());
    }
    if !(0.0..=1.0).contains(&alpha) {
        return Err("alpha must be between 0.0 and 1.0".to_string());
    }

    let mut blended = Vec::with_capacity(prev.data.len());
    let inv_alpha = 1.0 - alpha;
    for (&a, &b) in prev.data.iter().zip(next.data.iter()) {
        let value = (inv_alpha * a as f32 + alpha * b as f32).round();
        blended.push(value.clamp(0.0, 255.0) as u8);
    }

    Ok(RgbFrame {
        width: prev.width,
        height: prev.height,
        data: blended,
    })
}

pub fn interpolate_sequence(frames: &[RgbFrame], alpha: f32) -> Result<Vec<RgbFrame>, String> {
    if frames.is_empty() {
        return Ok(Vec::new());
    }

    let mut output = Vec::with_capacity(frames.len() * 2);
    output.push(frames[0].clone());
    for current in frames.iter().skip(1) {
        let prev = output
            .last()
            .ok_or_else(|| "Interpolation output missing previous frame".to_string())?;
        let mid = interpolate_rgb(prev, current, alpha)?;
        output.push(mid);
        output.push(current.clone());
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
pub struct MotionEstimationConfig {
    pub block_size: usize,
    pub search_radius: i32,
    pub alpha: f32,
}

impl MotionEstimationConfig {
    pub fn new(block_size: usize, search_radius: i32, alpha: f32) -> Result<Self, String> {
        if block_size == 0 {
            return Err("block_size must be greater than 0".to_string());
        }
        if search_radius < 0 {
            return Err("search_radius must be >= 0".to_string());
        }
        if !(0.0..=1.0).contains(&alpha) {
            return Err("alpha must be between 0.0 and 1.0".to_string());
        }
        Ok(Self {
            block_size,
            search_radius,
            alpha,
        })
    }
}

pub fn motion_compensated_blend(
    prev: &RgbFrame,
    next: &RgbFrame,
    config: MotionEstimationConfig,
) -> Result<RgbFrame, String> {
    if prev.width != next.width || prev.height != next.height {
        return Err("Frame dimensions must match for interpolation".to_string());
    }
    if prev.data.len() != next.data.len() {
        return Err("Frame data sizes must match for interpolation".to_string());
    }

    let width = prev.width as usize;
    let height = prev.height as usize;
    let block = config.block_size;
    let radius = config.search_radius;
    let alpha = config.alpha;

    let prev_luma = rgb_to_luma(&prev.data);
    let next_luma = rgb_to_luma(&next.data);

    let mut output = vec![0u8; prev.data.len()];

    for by in (0..height).step_by(block) {
        for bx in (0..width).step_by(block) {
            let bw = block.min(width - bx);
            let bh = block.min(height - by);

            let (dx, dy) = best_motion_vector(
                &prev_luma,
                &next_luma,
                width,
                height,
                bx,
                by,
                bw,
                bh,
                radius,
            );

            for y in 0..bh {
                for x in 0..bw {
                    let cx = bx + x;
                    let cy = by + y;
                    let src_x = clamp_i32(cx as i32 + dx, 0, (width - 1) as i32) as usize;
                    let src_y = clamp_i32(cy as i32 + dy, 0, (height - 1) as i32) as usize;

                    let dst_index = (cy * width + cx) * 3;
                    let src_index = (src_y * width + src_x) * 3;

                    for channel in 0..3 {
                        let a = prev.data[src_index + channel] as f32;
                        let b = next.data[dst_index + channel] as f32;
                        let value = (1.0 - alpha) * a + alpha * b;
                        output[dst_index + channel] = value.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }

    Ok(RgbFrame {
        width: prev.width,
        height: prev.height,
        data: output,
    })
}

fn rgb_to_luma(data: &[u8]) -> Vec<u8> {
    let mut luma = Vec::with_capacity(data.len() / 3);
    for rgb in data.chunks_exact(3) {
        let r = rgb[0] as f32;
        let g = rgb[1] as f32;
        let b = rgb[2] as f32;
        let value = 0.299 * r + 0.587 * g + 0.114 * b;
        luma.push(value.round().clamp(0.0, 255.0) as u8);
    }
    luma
}

fn best_motion_vector(
    prev_luma: &[u8],
    next_luma: &[u8],
    width: usize,
    height: usize,
    bx: usize,
    by: usize,
    bw: usize,
    bh: usize,
    radius: i32,
) -> (i32, i32) {
    let mut best_dx = 0;
    let mut best_dy = 0;
    let mut best_score = u64::MAX;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let score = block_sad(
                prev_luma,
                next_luma,
                width,
                height,
                bx,
                by,
                bw,
                bh,
                dx,
                dy,
            );
            if score < best_score {
                best_score = score;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    (best_dx, best_dy)
}

fn block_sad(
    prev_luma: &[u8],
    next_luma: &[u8],
    width: usize,
    height: usize,
    bx: usize,
    by: usize,
    bw: usize,
    bh: usize,
    dx: i32,
    dy: i32,
) -> u64 {
    let mut sum = 0u64;
    for y in 0..bh {
        let cy = by + y;
        let src_y = clamp_i32(cy as i32 + dy, 0, (height - 1) as i32) as usize;
        for x in 0..bw {
            let cx = bx + x;
            let src_x = clamp_i32(cx as i32 + dx, 0, (width - 1) as i32) as usize;
            let next_index = cy * width + cx;
            let prev_index = src_y * width + src_x;
            let diff = prev_luma[prev_index].abs_diff(next_luma[next_index]) as u64;
            sum += diff;
        }
    }
    sum
}

fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_single_pixel() {
        let prev = RgbFrame::new(1, 1, vec![0, 0, 0]).expect("prev frame");
        let next = RgbFrame::new(1, 1, vec![255, 128, 64]).expect("next frame");
        let blended = interpolate_rgb(&prev, &next, 0.5).expect("blend");
        assert_eq!(blended.data, vec![128, 64, 32]);
    }

    #[test]
    fn interpolate_sequence_inserts_midpoints() {
        let frame_a = RgbFrame::new(1, 1, vec![0, 0, 0]).expect("frame a");
        let frame_b = RgbFrame::new(1, 1, vec![100, 50, 25]).expect("frame b");
        let frame_c = RgbFrame::new(1, 1, vec![200, 100, 50]).expect("frame c");
        let output = interpolate_sequence(&[frame_a, frame_b, frame_c], 0.5).expect("sequence");
        assert_eq!(output.len(), 5);
        assert_eq!(output[1].data, vec![50, 25, 13]);
        assert_eq!(output[3].data, vec![150, 75, 38]);
    }

    #[test]
    fn motion_compensated_blend_aligns_simple_shift() {
        let prev = RgbFrame::new(2, 1, vec![255, 255, 255, 0, 0, 0]).expect("prev");
        let next = RgbFrame::new(2, 1, vec![0, 0, 0, 255, 255, 255]).expect("next");
        let config = MotionEstimationConfig::new(1, 1, 0.5).expect("config");
        let output = motion_compensated_blend(&prev, &next, config).expect("blend");
        assert_eq!(output.data, next.data);
    }
}
