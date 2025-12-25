use image::GenericImageView;

#[derive(Debug, Clone)]
pub struct CameraFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn fetch_camera_frame(url: &str) -> Result<CameraFrame, String> {
    let response = reqwest::blocking::get(url)
        .map_err(|e| format!("Camera request failed: {}", e))?;
    let bytes = response
        .bytes()
        .map_err(|e| format!("Failed to read camera response: {}", e))?;

    let jpeg_bytes = if looks_like_jpeg(&bytes) {
        bytes.to_vec()
    } else {
        extract_jpeg_from_mjpeg(&bytes)
            .ok_or_else(|| "Camera stream did not contain a JPEG frame".to_string())?
    };

    let image = image::load_from_memory(&jpeg_bytes)
        .map_err(|e| format!("Failed to decode camera frame: {}", e))?;
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();

    Ok(CameraFrame {
        data: rgb.into_raw(),
        width,
        height,
    })
}

fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8
}

fn extract_jpeg_from_mjpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut last_frame = None;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == 0xFF && bytes[i + 1] == 0xD8 {
            let start = i;
            let mut j = i + 2;
            while j + 1 < bytes.len() {
                if bytes[j] == 0xFF && bytes[j + 1] == 0xD9 {
                    let end = j + 2;
                    last_frame = Some(bytes[start..end].to_vec());
                    i = end;
                    break;
                }
                j += 1;
            }
        }
        i += 1;
    }
    last_frame
}
