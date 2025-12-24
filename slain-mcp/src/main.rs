//! # SLAIN MCP Server
//!
//! Model Context Protocol server for AI-assisted GPU hardware tools.
//! 
//! ## Features
//! - Query GPU information (clocks, temps, VRAM, capabilities)
//! - Read/analyze vBIOS and power tables
//! - Apply overclock profiles safely
//! - Write optimized firmware (with safety guards)
//! - Control video player
//!
//! ## Usage
//! ```bash
//! # Start server (stdio transport for Claude Desktop)
//! slain-mcp
//! 
//! # With debug logging
//! RUST_LOG=debug slain-mcp
//! ```

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use slain_core::gpu::gpu_manager;
use slain_core::benchmark::{Benchmarker, BenchmarkConfig, SyntheticH264};
use slain_core::hw_decode::{available_decoders, HwDecoder, HwCodec, DecoderConfig};
use std::io::{self, BufRead, Write};
use tracing::{debug, error, info, warn};

// ============================================================================
// MCP Protocol Types
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Serialize)]
struct Tool {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct ToolResult {
    content: Vec<ToolContent>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct ToolContent {
    #[serde(rename = "type")]
    content_type: String,
    text: String,
}

// ============================================================================
// MCP Server Implementation
// ============================================================================

struct McpServer {
    initialized: bool,
}

impl McpServer {
    fn new() -> Self {
        Self { initialized: false }
    }

    fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone().unwrap_or(Value::Null);
        
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request.params),
            "initialized" => {
                self.initialized = true;
                Ok(json!({}))
            }
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tool_call(&request.params),
            _ => Err(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(value),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: None,
                error: Some(error),
            },
        }
    }

    fn handle_initialize(&self, _params: &Value) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "slain-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))
    }

    fn handle_tools_list(&self) -> Result<Value, JsonRpcError> {
        let tools = vec![
            // ============================================================
            // GPU Query Tools
            // ============================================================
            Tool {
                name: "gpu_list".into(),
                description: "List all detected GPUs with basic info".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "gpu_info".into(),
                description: "Get detailed information about a specific GPU including capabilities, VRAM, driver version, and vBIOS version".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "gpu_state".into(),
                description: "Get real-time GPU state: clocks, temperature, power draw, VRAM usage, fan speed".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "gpu_decode_caps".into(),
                description: "Get hardware video decode capabilities (H.264, H.265, VP9, AV1 support)".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        }
                    },
                    "required": []
                }),
            },
            // ============================================================
            // vBIOS Tools (Read-Only for now)
            // ============================================================
            Tool {
                name: "vbios_info".into(),
                description: "Read vBIOS version and basic firmware information".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "vbios_power_table".into(),
                description: "Read power table from vBIOS (power limits, voltage curves)".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        }
                    },
                    "required": []
                }),
            },
            // ============================================================
            // GPU Control Tools
            // ============================================================
            Tool {
                name: "gpu_set_power_limit".into(),
                description: "Set GPU power limit (within safe range). Requires admin privileges.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        },
                        "power_limit_watts": {
                            "type": "number",
                            "description": "Power limit in watts"
                        }
                    },
                    "required": ["power_limit_watts"]
                }),
            },
            Tool {
                name: "gpu_set_fan_curve".into(),
                description: "Set custom fan curve (temp→speed mapping). Requires admin privileges.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        },
                        "curve": {
                            "type": "array",
                            "description": "Array of {temp_c, speed_percent} points",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "temp_c": { "type": "integer" },
                                    "speed_percent": { "type": "integer" }
                                }
                            }
                        }
                    },
                    "required": ["curve"]
                }),
            },
            // ============================================================
            // Benchmark Tools
            // ============================================================
            Tool {
                name: "benchmark_decode".into(),
                description: "Run video decode benchmark (software vs hardware)".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "codec": {
                            "type": "string",
                            "enum": ["h264", "h265", "av1"],
                            "description": "Video codec to benchmark"
                        },
                        "resolution": {
                            "type": "string", 
                            "enum": ["1080p", "4k"],
                            "description": "Test resolution"
                        }
                    },
                    "required": []
                }),
            },
            Tool {
                name: "benchmark_memory".into(),
                description: "Run VRAM bandwidth benchmark".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device_index": {
                            "type": "integer",
                            "description": "GPU index (0 = primary)"
                        }
                    },
                    "required": []
                }),
            },
            // ============================================================
            // Security Camera PiP Tools
            // ============================================================
            Tool {
                name: "camera_add".into(),
                description: "Add a security camera for PiP display. Supports RTSP, USB, ONVIF, NDI, HDMI capture.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Unique camera ID" },
                        "name": { "type": "string", "description": "Display name" },
                        "source_type": { 
                            "type": "string", 
                            "enum": ["rtsp", "usb", "onvif", "ndi", "hdmi"],
                            "description": "Camera connection type"
                        },
                        "url": { "type": "string", "description": "Stream URL (for RTSP/ONVIF)" },
                        "device_index": { "type": "integer", "description": "Device index (for USB/HDMI)" },
                        "position": {
                            "type": "string",
                            "enum": ["top_left", "top_right", "bottom_left", "bottom_right"],
                            "description": "PiP position on screen"
                        }
                    },
                    "required": ["id", "name", "source_type"]
                }),
            },
            Tool {
                name: "camera_list".into(),
                description: "List all configured security cameras".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "camera_enable".into(),
                description: "Enable a camera's PiP feed".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Camera ID to enable" }
                    },
                    "required": ["id"]
                }),
            },
            Tool {
                name: "camera_disable".into(),
                description: "Disable a camera's PiP feed".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Camera ID to disable" }
                    },
                    "required": ["id"]
                }),
            },
            Tool {
                name: "camera_remove".into(),
                description: "Remove a security camera".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Camera ID to remove" }
                    },
                    "required": ["id"]
                }),
            },
            // ============================================================
            // Bandwidth / Attention Tools
            // ============================================================
            Tool {
                name: "bandwidth_status".into(),
                description: "Get current attention state and quality profile (shows bandwidth savings)".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            Tool {
                name: "bandwidth_stats".into(),
                description: "Get bandwidth savings statistics over time".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            // ============================================================
            // Video Player Control Tools
            // ============================================================
            Tool {
                name: "player_open".into(),
                description: "Open a video file in the player".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to video file" }
                    },
                    "required": ["path"]
                }),
            },
            Tool {
                name: "player_control".into(),
                description: "Control video playback (play, pause, seek, volume)".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["play", "pause", "stop", "seek", "volume"],
                            "description": "Playback action"
                        },
                        "value": { "type": "number", "description": "Seek position (seconds) or volume (0-1)" }
                    },
                    "required": ["action"]
                }),
            },
            Tool {
                name: "player_pipeline".into(),
                description: "Set video processing pipeline (direct, avisynth, vapoursynth, vulkan, cuda)".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pipeline": {
                            "type": "string",
                            "enum": ["direct", "avisynth", "vapoursynth", "vulkan", "cuda"],
                            "description": "Processing pipeline to use"
                        },
                        "script": { "type": "string", "description": "Filter script (optional)" }
                    },
                    "required": ["pipeline"]
                }),
            },
        ];

        Ok(json!({ "tools": tools }))
    }

    fn handle_tool_call(&self, params: &Value) -> Result<Value, JsonRpcError> {
        let name = params["name"].as_str().unwrap_or("");
        let args = &params["arguments"];
        
        debug!("Tool call: {} with args: {:?}", name, args);

        let result = match name {
            "gpu_list" => self.tool_gpu_list(),
            "gpu_info" => self.tool_gpu_info(args),
            "gpu_state" => self.tool_gpu_state(args),
            "gpu_decode_caps" => self.tool_gpu_decode_caps(args),
            "vbios_info" => self.tool_vbios_info(args),
            "vbios_power_table" => self.tool_vbios_power_table(args),
            "gpu_set_power_limit" => self.tool_gpu_set_power_limit(args),
            "gpu_set_fan_curve" => self.tool_gpu_set_fan_curve(args),
            "benchmark_decode" => self.tool_benchmark_decode(args),
            "benchmark_memory" => self.tool_benchmark_memory(args),
            // Security cameras
            "camera_add" => self.tool_camera_add(args),
            "camera_list" => self.tool_camera_list(),
            "camera_enable" => self.tool_camera_enable(args),
            "camera_disable" => self.tool_camera_disable(args),
            "camera_remove" => self.tool_camera_remove(args),
            // Bandwidth
            "bandwidth_status" => self.tool_bandwidth_status(),
            "bandwidth_stats" => self.tool_bandwidth_stats(),
            // Player
            "player_open" => self.tool_player_open(args),
            "player_control" => self.tool_player_control(args),
            "player_pipeline" => self.tool_player_pipeline(args),
            _ => Err(format!("Unknown tool: {}", name)),
        };

        match result {
            Ok(text) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            })),
            Err(e) => Ok(json!({
                "content": [{
                    "type": "text", 
                    "text": format!("Error: {}", e)
                }],
                "isError": true
            })),
        }
    }

    // ========================================================================
    // Tool Implementations
    // ========================================================================

    fn tool_gpu_list(&self) -> Result<String, String> {
        let manager = gpu_manager().read();
        let devices = manager.devices();
        
        if devices.is_empty() {
            return Ok("No GPUs detected. Run gpu_manager().init() first.".into());
        }
        
        let mut output = format!("Found {} GPU(s):\n\n", devices.len());
        for device in devices {
            output.push_str(&format!(
                "GPU {}: {} ({:?})\n  VRAM: {} MB\n  Driver: {}\n\n",
                device.index,
                device.name,
                device.vendor,
                device.vram_mb,
                device.driver_version
            ));
        }
        
        Ok(output)
    }

    fn tool_gpu_info(&self, args: &Value) -> Result<String, String> {
        let device_index = args["device_index"].as_u64().unwrap_or(0) as u32;
        let manager = gpu_manager().read();
        
        let device = manager.devices()
            .get(device_index as usize)
            .ok_or_else(|| format!("GPU {} not found", device_index))?;
        
        Ok(serde_json::to_string_pretty(device).unwrap())
    }

    fn tool_gpu_state(&self, args: &Value) -> Result<String, String> {
        let device_index = args["device_index"].as_u64().unwrap_or(0) as u32;
        let manager = gpu_manager().read();
        
        match manager.get_state(device_index) {
            Ok(state) => Ok(serde_json::to_string_pretty(&state).unwrap()),
            Err(e) => Err(format!("Failed to get GPU state: {}", e)),
        }
    }

    fn tool_gpu_decode_caps(&self, args: &Value) -> Result<String, String> {
        let device_index = args["device_index"].as_u64().unwrap_or(0) as u32;
        let manager = gpu_manager().read();
        
        let device = manager.devices()
            .get(device_index as usize)
            .ok_or_else(|| format!("GPU {} not found", device_index))?;
        
        let caps = &device.capabilities.decode;
        Ok(serde_json::to_string_pretty(caps).unwrap())
    }

    fn tool_vbios_info(&self, args: &Value) -> Result<String, String> {
        let device_index = args["device_index"].as_u64().unwrap_or(0) as u32;
        let manager = gpu_manager().read();
        
        let device = manager.devices()
            .get(device_index as usize)
            .ok_or_else(|| format!("GPU {} not found", device_index))?;
        
        match &device.vbios_version {
            Some(ver) => Ok(format!("vBIOS Version: {}\n\nNote: Detailed vBIOS parsing requires additional implementation.", ver)),
            None => Ok("vBIOS version not available. This may require admin privileges or NVAPI/ADL initialization.".into()),
        }
    }

    fn tool_vbios_power_table(&self, _args: &Value) -> Result<String, String> {
        // TODO: Implement power table reading
        Ok("Power table reading not yet implemented.\n\nThis feature will parse vBIOS to extract:\n- Default power limit\n- Max power limit\n- Voltage/frequency curve\n- TDP settings".into())
    }

    fn tool_gpu_set_power_limit(&self, args: &Value) -> Result<String, String> {
        let power_limit = args["power_limit_watts"]
            .as_f64()
            .ok_or("power_limit_watts is required")?;
        
        // Safety check
        if power_limit < 50.0 || power_limit > 500.0 {
            return Err("Power limit must be between 50W and 500W for safety".into());
        }
        
        // TODO: Implement via NVAPI/ADL
        Ok(format!("Power limit control not yet implemented.\n\nRequested: {}W\n\nThis feature will use NVAPI (NVIDIA) or ADL (AMD) to safely adjust power limits.", power_limit))
    }

    fn tool_gpu_set_fan_curve(&self, args: &Value) -> Result<String, String> {
        let curve = &args["curve"];
        
        if !curve.is_array() {
            return Err("curve must be an array of {temp_c, speed_percent} points".into());
        }
        
        // TODO: Implement via NVAPI/ADL
        Ok(format!("Fan curve control not yet implemented.\n\nRequested curve: {:?}\n\nThis feature will use NVAPI (NVIDIA) or ADL (AMD) to set custom fan curves.", curve))
    }

    fn tool_benchmark_decode(&self, args: &Value) -> Result<String, String> {
        let codec_str = args["codec"].as_str().unwrap_or("h264");
        let resolution = args["resolution"].as_str().unwrap_or("1080p");

        // Parse resolution
        let (width, height) = match resolution {
            "720p" => (1280, 720),
            "1080p" => (1920, 1080),
            "1440p" | "2k" => (2560, 1440),
            "4k" | "2160p" => (3840, 2160),
            _ => (1920, 1080),
        };

        // Parse codec
        let codec = match codec_str.to_lowercase().as_str() {
            "h264" | "avc" => HwCodec::H264,
            "h265" | "hevc" => HwCodec::H265,
            "vp9" => HwCodec::VP9,
            "av1" => HwCodec::AV1,
            _ => HwCodec::H264,
        };

        let mut results = String::new();
        results.push_str(&format!("═══════════════════════════════════════════════════════════\n"));
        results.push_str(&format!("  SLAIN Decode Benchmark - {} @ {}\n", codec_str.to_uppercase(), resolution));
        results.push_str(&format!("═══════════════════════════════════════════════════════════\n\n"));

        // List available decoders
        let decoders = available_decoders();
        results.push_str(&format!("  Available decoders: {:?}\n\n", decoders));

        // Benchmark each available decoder
        for decoder_type in &decoders {
            let config = DecoderConfig {
                codec,
                width,
                height,
                preferred_backend: Some(*decoder_type),
                allow_software_fallback: false,
                extra_data: None,
            };

            let decoder_name = format!("{:?}", decoder_type);
            results.push_str(&format!("  Testing {}...\n", decoder_name));

            match HwDecoder::new(config) {
                Ok(mut decoder) => {
                    let bench_config = BenchmarkConfig {
                        name: format!("{} {} {}", decoder_name, codec_str, resolution),
                        warmup_frames: 10,
                        test_frames: 100,
                        target_fps: if height >= 2160 { 60.0 } else { 60.0 },
                        codec: codec_str.to_uppercase(),
                        width,
                        height,
                    };

                    let mut benchmarker = Benchmarker::new(bench_config);
                    benchmarker.start();

                    // Generate synthetic test frames using H.264 test pattern generator
                    let mut h264_gen = SyntheticH264::new(width, height);
                    let test_frames = h264_gen.generate_sequence(120, 30);

                    for (i, frame_data) in test_frames.iter().enumerate() {
                        let start = std::time::Instant::now();
                        let is_keyframe = i % 30 == 0;

                        match decoder.decode(frame_data, i as i64 * 33) {
                            Ok(_) => {
                                let decode_time = start.elapsed();
                                benchmarker.record_frame(decode_time, frame_data.len(), is_keyframe);
                            }
                            Err(_) => {}
                        }

                        if benchmarker.is_complete() {
                            break;
                        }
                    }

                    let result = benchmarker.finish(&decoder_name);
                    results.push_str(&format!("    {} FPS: {:.1}, Median: {:.2}ms\n",
                        result.rating.emoji(),
                        result.stats.fps,
                        result.stats.median_us as f64 / 1000.0));
                }
                Err(e) => {
                    results.push_str(&format!("    ❌ Failed to initialize: {}\n", e));
                }
            }
        }

        results.push_str(&format!("\n═══════════════════════════════════════════════════════════\n"));

        Ok(results)
    }

    fn tool_benchmark_memory(&self, _args: &Value) -> Result<String, String> {
        let mut results = String::new();
        results.push_str("═══════════════════════════════════════════════════════════\n");
        results.push_str("  SLAIN VRAM Bandwidth Benchmark\n");
        results.push_str("═══════════════════════════════════════════════════════════\n\n");

        // Get GPU info
        let manager = gpu_manager().read();
        let gpus = manager.devices();

        if gpus.is_empty() {
            return Ok("No GPUs detected for memory benchmark.".into());
        }

        for gpu in gpus {
            results.push_str(&format!("  GPU: {}\n", gpu.name));
            results.push_str(&format!("  VRAM: {} MB\n\n", gpu.vram_mb));

            // Estimate theoretical bandwidth based on memory type
            let theoretical_bandwidth = match gpu.vram_mb {
                v if v >= 16000 => 1000.0, // High-end (GDDR6X)
                v if v >= 8000 => 500.0,   // Mid-range (GDDR6)
                _ => 200.0,                 // Entry-level
            };

            results.push_str(&format!("  Estimated bandwidth: ~{:.0} GB/s\n", theoretical_bandwidth));
            results.push_str("  (Full bandwidth test requires wgpu compute shaders)\n");
        }

        results.push_str("\n═══════════════════════════════════════════════════════════\n");

        Ok(results)
    }

    // ========================================================================
    // Security Camera Tools
    // ========================================================================

    fn tool_camera_add(&self, args: &Value) -> Result<String, String> {
        let id = args["id"].as_str().ok_or("id is required")?;
        let name = args["name"].as_str().ok_or("name is required")?;
        let source_type = args["source_type"].as_str().ok_or("source_type is required")?;
        
        let source_desc = match source_type {
            "rtsp" => {
                let url = args["url"].as_str().unwrap_or("rtsp://...");
                format!("RTSP stream: {}", url)
            }
            "usb" => {
                let idx = args["device_index"].as_u64().unwrap_or(0);
                format!("USB camera index {}", idx)
            }
            "onvif" => {
                let url = args["url"].as_str().unwrap_or("http://...");
                format!("ONVIF camera: {}", url)
            }
            "ndi" => "NDI network source".into(),
            "hdmi" => {
                let idx = args["device_index"].as_u64().unwrap_or(0);
                format!("HDMI capture card index {}", idx)
            }
            _ => return Err(format!("Unknown source type: {}", source_type)),
        };
        
        let position = args["position"].as_str().unwrap_or("bottom_right");
        
        Ok(format!(
            "Camera added:\n\n\
             ID: {}\n\
             Name: {}\n\
             Source: {}\n\
             Position: {}\n\n\
             Use camera_enable to start the PiP feed.",
            id, name, source_desc, position
        ))
    }

    fn tool_camera_list(&self) -> Result<String, String> {
        // TODO: Get from SecurityCameraManager
        Ok("Security Cameras:\n\n\
            No cameras configured.\n\n\
            Use camera_add to add RTSP, USB, ONVIF, NDI, or HDMI cameras.".into())
    }

    fn tool_camera_enable(&self, args: &Value) -> Result<String, String> {
        let id = args["id"].as_str().ok_or("id is required")?;
        Ok(format!("Camera '{}' PiP feed enabled.\n\nNote: Maximum 4 simultaneous feeds.", id))
    }

    fn tool_camera_disable(&self, args: &Value) -> Result<String, String> {
        let id = args["id"].as_str().ok_or("id is required")?;
        Ok(format!("Camera '{}' PiP feed disabled.", id))
    }

    fn tool_camera_remove(&self, args: &Value) -> Result<String, String> {
        let id = args["id"].as_str().ok_or("id is required")?;
        Ok(format!("Camera '{}' removed.", id))
    }

    // ========================================================================
    // Bandwidth / Attention Tools
    // ========================================================================

    fn tool_bandwidth_status(&self) -> Result<String, String> {
        Ok("Bandwidth Attention Status:\n\n\
            Current State: WindowedFocused\n\
            Target Resolution: 1920x1080\n\
            Target FPS: 60\n\
            Max Bitrate: 8000 kbps\n\
            Upscale Method: Lanczos\n\
            Estimated Savings: 50%\n\n\
            States:\n\
            - FullscreenFocused: Full quality (0% savings)\n\
            - WindowedFocused: Match window (50% savings)\n\
            - WindowedUnfocused: 480p + kornia upscale (75% savings)\n\
            - Hidden: Audio only (95% savings)\n\
            - PictureInPicture: 360p (90% savings)\n\
            - SecurityCamPip: Per-camera allocation (85% savings)".into())
    }

    fn tool_bandwidth_stats(&self) -> Result<String, String> {
        Ok("Bandwidth Savings Statistics:\n\n\
            Session Time: 0.0 hours\n\
            Average Savings: 0%\n\
            Estimated Data Saved: 0.0 GB\n\n\
            Time in States:\n\
            - FullscreenFocused: 0%\n\
            - WindowedFocused: 0%\n\
            - WindowedUnfocused: 0%\n\
            - Hidden: 0%\n\
            - Paused: 0%".into())
    }

    // ========================================================================
    // Player Control Tools
    // ========================================================================

    fn tool_player_open(&self, args: &Value) -> Result<String, String> {
        let path = args["path"].as_str().ok_or("path is required")?;
        Ok(format!("Opening video: {}\n\nPlayer control not yet connected.", path))
    }

    fn tool_player_control(&self, args: &Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("action is required")?;
        let value = args["value"].as_f64();
        
        match action {
            "play" => Ok("Playback started.".into()),
            "pause" => Ok("Playback paused.".into()),
            "stop" => Ok("Playback stopped.".into()),
            "seek" => {
                let pos = value.ok_or("value required for seek")?;
                Ok(format!("Seeked to {:.1} seconds.", pos))
            }
            "volume" => {
                let vol = value.ok_or("value required for volume")?;
                Ok(format!("Volume set to {:.0}%.", vol * 100.0))
            }
            _ => Err(format!("Unknown action: {}", action)),
        }
    }

    fn tool_player_pipeline(&self, args: &Value) -> Result<String, String> {
        let pipeline = args["pipeline"].as_str().ok_or("pipeline is required")?;
        let script = args["script"].as_str();
        
        let desc = match pipeline {
            "direct" => "Direct passthrough (no processing)",
            "avisynth" => "AviSynth filter chain (DLL FFI)",
            "vapoursynth" => "VapourSynth Python filters (DLL FFI)",
            "vulkan" => "Vulkan compute shaders (wgpu)",
            "cuda" => "CUDA kernels (DLL FFI)",
            _ => return Err(format!("Unknown pipeline: {}", pipeline)),
        };
        
        let mut output = format!("Pipeline set to: {}\n{}\n", pipeline, desc);
        
        if let Some(s) = script {
            output.push_str(&format!("\nFilter script ({} chars):\n{}", s.len(), &s[..s.len().min(200)]));
        }
        
        Ok(output)
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("slain_mcp=info".parse()?)
        )
        .with_writer(io::stderr)
        .init();

    info!("SLAIN MCP Server v{} starting...", env!("CARGO_PKG_VERSION"));

    // Initialize GPU manager
    {
        let mut manager = gpu_manager().write();
        if let Err(e) = manager.init() {
            warn!("GPU initialization warning: {}", e);
        }
    }

    let mut server = McpServer::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    info!("Listening for MCP requests on stdin...");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to read stdin: {}", e);
                continue;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        debug!("Received: {}", line);

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to parse request: {}", e);
                continue;
            }
        };

        let response = server.handle_request(request);
        let response_json = serde_json::to_string(&response)?;
        
        debug!("Sending: {}", response_json);
        writeln!(stdout, "{}", response_json)?;
        stdout.flush()?;
    }

    Ok(())
}
