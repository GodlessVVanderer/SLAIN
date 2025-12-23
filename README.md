# SLAIN

A GPU-accelerated video player written in pure Rust.

## What It Does

SLAIN plays video files using your GPU's hardware decoder when available. This means smoother playback and lower CPU usage compared to software decoding.

**Supported formats:** MP4, MKV, AVI, TS  
**Supported codecs:** H.264, H.265/HEVC, VP9, AV1  
**Supported GPUs:** NVIDIA (NVDEC), AMD (AMF), Intel (VA-API on Linux)

## Running SLAIN

SLAIN is portable. No installation required.

### Windows

1. Extract the zip
2. Double-click `slain-player.exe`
3. Drag a video file onto the window, or use File → Open

### Linux

```bash
./slain-player
```

### Requirements

- A GPU with hardware video decoding support
- For NVIDIA: Driver 450+ recommended
- For AMD: Adrenalin driver
- For software fallback: No special requirements

## Controls

| Key | Action |
|-----|--------|
| Space | Play / Pause |
| Left/Right | Seek ±5 seconds |
| Up/Down | Volume |
| F | Fullscreen |
| M | Mute |
| Escape | Exit fullscreen |
| O | On-screen display toggle |
| I | Show video info |

Mouse wheel adjusts volume by default.

## What You See

When you open a video:

1. The video plays in the main window
2. Bottom bar shows playback position
3. Press `O` to see stats overlay (FPS, decoder type, frame drops)
4. Press `I` for detailed codec/resolution info

## Technical Notes

- Written entirely in Rust
- Uses wgpu for GPU rendering
- Hardware decoding via native OS APIs (no FFmpeg dependency)
- Falls back to software decoding if hardware unavailable

## Building from Source

```bash
cargo build --release
```

The binary will be in `target/release/slain-player`.

### Optional Dependencies

For H.264 software fallback, place `openh264.dll` (Windows) or `libopenh264.so` (Linux) alongside the executable. Available from [Cisco's OpenH264 releases](https://github.com/cisco/openh264/releases).

## Troubleshooting

**Video doesn't play:**
- Check the console output for decoder errors
- Try a different video file to isolate the issue
- Ensure your GPU drivers are up to date

**No hardware acceleration:**
- SLAIN will fall back to software decoding automatically
- Check `O` overlay to see which decoder is active

**Choppy playback:**
- Press `O` to check frame drop count
- Try reducing window size
- Ensure no other GPU-intensive applications are running

## License

MIT
