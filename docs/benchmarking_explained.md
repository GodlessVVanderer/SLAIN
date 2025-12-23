# Understanding SLAIN Benchmarks

This document explains what the benchmarks measure and what the results mean, without technical jargon.

## What Is a Benchmark?

A benchmark is a standardized test that measures how fast your computer can do a specific task. In SLAIN, benchmarks measure how quickly your system can decode (process) video.

Think of it like a timed typing test—it measures speed under consistent conditions so you can compare results.

## Why Benchmark Video Decoding?

Video files are compressed. Before you can watch them, your computer must decompress each frame. This happens constantly during playback—24 to 60+ times per second.

If your system can't decode frames fast enough, you'll see:
- Stuttering or choppy video
- Audio getting out of sync
- Dropped frames

Benchmarking tells you how much headroom your system has before problems occur.

## Hardware vs. Software Decoding

### Hardware Decoding

Modern GPUs have dedicated circuits designed specifically for video decoding. This is called hardware decoding.

**Names you might see:**
- NVDEC (NVIDIA)
- AMF/VCN (AMD)
- Quick Sync (Intel)
- VA-API (Linux)

**Advantages:**
- Very fast
- Uses almost no CPU
- Power efficient

**When it's used:**
- Common video formats (H.264, H.265, VP9)
- Standard resolutions up to 8K (GPU dependent)

### Software Decoding

Software decoding uses your CPU to decompress video, running the same math that hardware would do, but as regular code.

**Why does software decoding exist?**

1. **Compatibility:** Not every GPU supports every codec. Older GPUs might not decode H.265 or AV1. Software decoding works everywhere.

2. **Unusual formats:** Some rare or very old codecs have no hardware support. Software is the only option.

3. **Accuracy:** For professional work, software decoding can sometimes be more accurate or offer more options.

**Tradeoffs:**
- Slower than hardware
- Uses significant CPU
- Uses more power (laptop battery drains faster)

SLAIN uses hardware decoding when available and falls back to software automatically.

## Reading Benchmark Results

When you run a benchmark, you'll see results like this:

```
Frames decoded: 1,798
Average FPS: 299.7
Median decode time: 3.12ms
99th percentile: 4.87ms
```

### What Each Number Means

**Frames decoded**  
How many video frames were processed during the test. More frames = more reliable results.

**Average FPS (Frames Per Second)**  
How many frames per second your system decoded, on average. 

- 24 FPS = Minimum for film playback
- 30 FPS = Standard video
- 60 FPS = High frame rate video
- 120+ FPS = You have significant headroom

If this number is higher than your video's frame rate, playback will be smooth.

**Median decode time**  
Half of all frames decoded faster than this time, half slower. This is more reliable than average because it ignores occasional slow frames.

- Under 10ms = Excellent for 60fps content
- Under 20ms = Good for 30fps content
- Under 40ms = Acceptable for 24fps content

**99th percentile**  
99% of frames decoded faster than this time. This shows your "worst case" performance. If this number is too high, you might see occasional stutters even if average performance is good.

### The Rating

SLAIN gives a simple rating based on how your results compare to playback requirements:

- **Excellent:** Decodes at 3x+ realtime speed
- **Good:** Decodes at 1.5-3x realtime speed  
- **Acceptable:** Decodes at 1-1.5x realtime speed
- **Poor:** Struggles to keep up with realtime playback

## Comparing Your Results

### Comparing Over Time

Run the same benchmark periodically to check for:
- Driver updates improving performance
- System degradation (thermal throttling, etc.)
- Effects of other software

### Comparing Across Systems

Benchmark results can be compared between different computers, but keep in mind:

- Same codec and resolution must be used
- Same benchmark duration for reliability
- Different GPUs will show very different results

### What Affects Results

**Things that improve results:**
- Newer GPU
- Updated drivers
- Faster CPU (for software decode)
- Cooler temperatures (less throttling)

**Things that hurt results:**
- Running other programs simultaneously
- Laptop on battery (power saving modes)
- Overheating
- Outdated drivers

## Common Questions

**Q: My hardware benchmark failed. What's wrong?**

Your GPU might not support the codec being tested. This is normal—SLAIN will use software decoding for that format. Try benchmarking a different codec.

**Q: Software decoding is faster than hardware on my system. Is that normal?**

Unusual but possible with very fast CPUs and older GPUs. Hardware decoding still uses less power and frees your CPU for other tasks.

**Q: My results vary each time I run the benchmark.**

Some variation is normal (±5%). Larger variations might indicate:
- Background programs interfering
- Thermal throttling (run tests with cool system)
- Power management changing clock speeds

**Q: What's a "good" result?**

It depends on what you're playing:
- 1080p 30fps video: Anything over 60 FPS decode rate is plenty
- 4K 60fps video: You want 120+ FPS decode rate for smooth playback
- 8K video: Hardware decode almost mandatory

**Q: Should I disable software fallback?**

Generally no. Software fallback ensures you can always play videos, even if hardware doesn't support the format. Only disable it if you're troubleshooting or have a specific reason.
