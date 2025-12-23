# SLAIN User Guide

## First Launch

When you first open SLAIN, you see an empty player window with a dark background. There are three ways to open a video:

1. **Drag and drop** a file onto the window
2. **File menu** → Open (Ctrl+O)
3. **Command line:** `slain-player video.mp4`

## The Player Interface

### Main View

```
┌─────────────────────────────────────────┐
│                                         │
│                                         │
│              Video Area                 │
│                                         │
│                                         │
├─────────────────────────────────────────┤
│ advancement bar                        │
│ ▶ 01:23  advancement slider     02:45   │
└─────────────────────────────────────────┘
```

- Click anywhere on the video to play/pause
- Move your mouse to show controls
- Controls auto-hide after 3 seconds of inactivity

### On-Screen Display (OSD)

Press `O` to toggle the stats overlay:

```
┌─────────────────────────┐
│ FPS: 59.94              │
│ Decoder: NVDEC          │
│ Resolution: 1920x1080   │
│ Dropped: 0              │
│ Buffer: 24 frames       │
└─────────────────────────┘
```

This shows you:
- **FPS:** Actual playback frame rate
- **Decoder:** Which decoder is active (NVDEC, AMF, VAAPI, or Software)
- **Resolution:** Video dimensions
- **Dropped:** Frames skipped to maintain sync
- **Buffer:** Decoded frames waiting to display

### Video Info Panel

Press `I` for detailed information:

```
┌─────────────────────────────────┐
│ File: movie.mkv                 │
│ Container: Matroska             │
│ Video: H.265 1920x1080 23.976fps│
│ Audio: AAC 48000Hz stereo       │
│ Duration: 1:42:15               │
│ Bitrate: 8.2 Mbps               │
└─────────────────────────────────┘
```

## Settings

Access settings via the menu bar or press `Ctrl+,`

### Video Settings

| Setting | Options | Default |
|---------|---------|---------|
| Hardware Decode | Auto / Force / Disable | Auto |
| Deinterlace | Auto / Off / Bob / Yadif | Auto |
| Color Range | Auto / Limited / Full | Auto |

**Hardware Decode options:**
- **Auto:** Try hardware first, fall back to software
- **Force:** Only use hardware (fail if unavailable)
- **Disable:** Always use software decoding

### Audio Settings

| Setting | Options | Default |
|---------|---------|---------|
| Output Device | System default / [List] | System default |
| Volume | 0-150% | 100% |
| Normalize | On / Off | Off |

### Interface Settings

| Setting | Options | Default |
|---------|---------|---------|
| OSD Position | Top-left / Top-right / Bottom-left / Bottom-right | Top-left |
| OSD Opacity | 0-100% | 80% |
| Seek Increment | 5s / 10s / 30s | 5s |
| Mouse Wheel | Volume / Seek | Volume |

## Benchmarking

SLAIN includes a built-in benchmark to measure your system's video decode performance.

### Running a Benchmark

1. Open the menu: **Tools → Benchmark**
2. Select benchmark type:
   - **Quick:** 10-second test
   - **Standard:** 60-second test
   - **Extended:** 5-minute test
3. Click **Run**

### Benchmark Results

After the benchmark completes, you see:

```
┌─────────────────────────────────────┐
│ Benchmark Results                   │
├─────────────────────────────────────┤
│ Decoder: NVDEC                      │
│ Codec: H.264 1080p                  │
│                                     │
│ Frames decoded: 1,798               │
│ Average FPS: 299.7                  │
│ Median decode time: 3.12ms          │
│ 99th percentile: 4.87ms             │
│                                     │
│ Rating: Excellent                   │
│ Your GPU decodes 1080p H.264 at     │
│ ~5x realtime speed.                 │
└─────────────────────────────────────┘
```

Results are saved to `benchmarks/` folder as JSON files for later comparison.

### Comparing Results

Go to **Tools → Benchmark History** to see past results. You can:
- Sort by date, decoder, or performance
- Export results as CSV
- Delete old results

## MCP Integration (Optional)

SLAIN can optionally expose playback controls via the Model Context Protocol (MCP). This allows AI assistants or automation tools to control the player.

### What MCP Does

When enabled, external tools can:
- Play, pause, seek
- Get playback status
- Query video metadata
- Run benchmarks

### What MCP Does NOT Do

MCP cannot:
- Access files outside the player
- Modify system settings
- Control hardware directly
- Access the network

### Enabling MCP

MCP is **disabled by default**. To enable:

1. Go to **Settings → Advanced → MCP Server**
2. Toggle **Enable MCP Server**
3. Set the port (default: 3000)
4. Optionally set an access token

```
┌─────────────────────────────────────┐
│ MCP Server Settings                 │
├─────────────────────────────────────┤
│ [x] Enable MCP Server               │
│                                     │
│ Port: [3000]                        │
│                                     │
│ [ ] Require Access Token            │
│ Token: [________________]           │
│                                     │
│ Status: Running on localhost:3000   │
└─────────────────────────────────────┘
```

### MCP Security

- MCP only listens on localhost by default
- Network access must be explicitly enabled
- Token authentication is recommended for any non-local use

## Keyboard Reference

### Playback

| Key | Action |
|-----|--------|
| Space | Play / Pause |
| Enter | Play / Pause |
| S | Stop |

### Seeking

| Key | Action |
|-----|--------|
| Left | Back 5 seconds |
| Right | Forward 5 seconds |
| Shift+Left | Back 30 seconds |
| Shift+Right | Forward 30 seconds |
| Home | Go to start |
| End | Go to end |

### Volume

| Key | Action |
|-----|--------|
| Up | Volume +5% |
| Down | Volume -5% |
| M | Mute toggle |

### Display

| Key | Action |
|-----|--------|
| F | Fullscreen toggle |
| Escape | Exit fullscreen |
| O | OSD toggle |
| I | Info panel toggle |

### Other

| Key | Action |
|-----|--------|
| Ctrl+O | Open file |
| Ctrl+Q | Quit |
| Ctrl+, | Settings |
| ? | Show keyboard shortcuts |
