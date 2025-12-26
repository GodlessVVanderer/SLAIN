# SLAIN Video Player - Development Status Update

**To:** Interested Parties / Potential Partners  
**From:** Josh Davidson  
**Subject:** SLAIN - Pure Rust GPU-Accelerated Video Player & Media Platform  
**Date:** December 2024

---

## Executive Summary

SLAIN is a next-generation video player built entirely in Rust, designed for maximum performance, security, and extensibility. Unlike traditional players that rely on FFmpeg, SLAIN implements its own pure Rust decoding pipelines with direct GPU hardware acceleration. The application is being developed for public distribution as a standalone, portable package.

---

## Current Capabilities

### Core Video Playback

| Feature | Status | Description |
|---------|--------|-------------|
| **Hardware Decoding** | ✅ Complete | NVDEC (NVIDIA), AMF (AMD), VA-API (Linux) |
| **Software Decoding** | ✅ Complete | Pure Rust H.264 decoder for CPU baseline |
| **Container Support** | ✅ Complete | MKV, MP4, AVI, MPEG-TS demuxers |
| **Audio Playback** | ✅ Complete | Symphonia-based decoding with A/V sync |
| **Subtitle Rendering** | ✅ Complete | SRT, ASS/SSA support |

### GPU Processing Pipelines

SLAIN features a unique multi-pipeline architecture allowing real-time video processing:

- **Direct Pipeline** - Zero-copy passthrough for maximum performance
- **Software Pipeline** - CPU-based processing (benchmark baseline)
- **Vulkan Pipeline** - wgpu compute shaders for color correction, sharpening, denoising
- **CUDA Pipeline** - Native NVIDIA PTX kernels via driver API
- **AviSynth Pipeline** - Windows scripting integration
- **VapourSynth Pipeline** - Cross-platform Python scripting

### Streaming & Network

- YouTube playback with Discord proxy support
- IPTV/M3U playlist support
- Debrid service integration (Real-Debrid, AllDebrid)
- Watch party synchronization
- Security camera PIP overlay

### Special Features

- **RetroTV Mode** - CRT scanlines, VHS tracking, analog noise effects
- **Aegis Security** - Content protection and DRM framework
- **Benchmark System v3** - Three-way visual comparison charts (Software/Before/Current)
- **Real-time OSD Statistics** - Frame times, GPU utilization, decode latency

---

## FORUMYZE Integration (NEW)

SLAIN now includes **FORUMYZE**, a YouTube comment analysis system rewritten entirely in Rust:

### Free Features (User Provides Own API Key)
- YouTube Data API v3 integration
- Comment fetching with pagination (up to 100,000 comments)
- Intelligent spam/garbage filtering
- AI-powered categorization (Gemini API or local heuristics)
- Discussion topic extraction
- Sentiment analysis

### Legal Evidence Packages (Premium Service for Law Firms)
- **Witness Statement Extraction** - Identifies eyewitness accounts, first-hand knowledge, expert commentary
- **Claim Analysis** - Extracts observations, temporal references, contradictions to official narratives
- **CourtListener Integration** - Automatic case law research (Section 1983, qualified immunity, excessive force)
- **Timeline Reconstruction** - Builds chronological event sequences from public testimony
- **Export Formats** - JSON, Markdown, PDF, DOCX for court filings

### Persistent Message Board
- SQLite-backed discussion forum per video
- Google OAuth authentication
- Threads, posts, reactions (upvote, heart, "witnessed" 👁️)
- Import FORUMYZE discussions directly
- Offline-first with optional sync

---

## Technical Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      SLAIN PLAYER                           │
│                    (Rust + wgpu)                            │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │   DEMUXER   │  │   DECODER   │  │  RENDERER   │         │
│  │ MKV/MP4/AVI │→ │ NVDEC/AMF/  │→ │   wgpu +    │         │
│  │   TS/LAV    │  │ VAAPI/SW    │  │  Vulkan     │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │  FORUMYZE   │  │   LEGAL     │  │   MESSAGE   │         │
│  │  YouTube    │→ │  EVIDENCE   │→ │    BOARD    │         │
│  │  Analysis   │  │  Packages   │  │   SQLite    │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         │
│  │  STREAMING  │  │   IPTV      │  │   RETRO     │         │
│  │  YouTube/   │  │   M3U       │  │    TV       │         │
│  │  Debrid     │  │  Playlists  │  │  Effects    │         │
│  └─────────────┘  └─────────────┘  └─────────────┘         │
└─────────────────────────────────────────────────────────────┘
```

---

## Coming Soon: 3D Scan Viewing & Rendering

The next major feature for SLAIN is **3D scan visualization**:

### Planned Capabilities
- **Format Support** - PLY, OBJ, STL, glTF/GLB, E57 point clouds
- **Point Cloud Rendering** - GPU-accelerated billions of points via wgpu
- **Mesh Visualization** - Textured mesh display with PBR materials
- **Photogrammetry Integration** - View reconstructions from drone/phone scans
- **Measurement Tools** - Distance, area, volume calculations
- **Annotation System** - Add markers, notes, highlights to 3D models
- **VR/AR Ready** - OpenXR integration for immersive viewing
- **Evidence Documentation** - 3D crime scene / accident reconstruction viewing

### Use Cases
- Crime scene documentation review
- Accident reconstruction visualization  
- Archaeological site exploration
- Real estate virtual tours
- Medical imaging (CT/MRI surface renders)
- Industrial inspection documentation

This positions SLAIN not just as a video player, but as a comprehensive **visual evidence platform** for legal, forensic, and professional applications.

---

## Technology Stack

| Component | Technology |
|-----------|------------|
| Language | Rust (100% safe, no FFmpeg dependency) |
| GPU | wgpu (Vulkan/DX12/Metal), CUDA PTX |
| Audio | Symphonia + cpal + rubato |
| Database | SQLite (rusqlite) |
| HTTP | reqwest (async) |
| Serialization | serde + serde_json |
| Window | winit |

---

## Distribution

SLAIN will be distributed as a **single portable executable** with no external dependencies. The pure Rust architecture ensures:

- No DLL hell
- No codec pack requirements  
- Cross-platform (Windows, Linux, macOS)
- Minimal attack surface
- Reproducible builds

---

## Contact

For partnership inquiries, licensing discussions, or early access:

**Repository:** https://github.com/GodlessVVanderer/SLAIN  
**Developer:** Josh Davidson

---

*SLAIN - See Everything. Understand Everything.*
