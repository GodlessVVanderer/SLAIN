#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any, Dict, List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run headless playback fixtures.")
    parser.add_argument(
        "--fixtures",
        default="slain-player/fixtures/fixture_matrix.json",
        help="Path to fixture matrix JSON.",
    )
    parser.add_argument(
        "--suite",
        default="smoke",
        choices=["smoke", "full"],
        help="Which fixture suite to run.",
    )
    parser.add_argument(
        "--log-dir",
        default="artifacts/headless-logs",
        help="Directory to write per-fixture logs.",
    )
    parser.add_argument(
        "--report",
        default="artifacts/headless-report.md",
        help="Path to write summary report.",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="Skip building the slain-player binary.",
    )
    parser.add_argument(
        "--release",
        action="store_true",
        help="Use --release build for running fixtures.",
    )
    return parser.parse_args()


def load_fixtures(path: Path) -> List[Dict[str, Any]]:
    data = json.loads(path.read_text())
    fixtures = data.get("fixtures", [])
    if not fixtures:
        raise RuntimeError("No fixtures found in matrix file.")
    return fixtures


def ensure_download(fixture: Dict[str, Any], downloads_dir: Path) -> Path:
    filename = fixture.get("file")
    url = fixture.get("url")
    if not filename or not url:
        raise RuntimeError(f"Fixture {fixture.get('id')} missing file or url.")

    downloads_dir.mkdir(parents=True, exist_ok=True)
    target_path = downloads_dir / filename
    if target_path.exists():
        return target_path

    print(f"Downloading {fixture['id']} from {url}...")
    urllib.request.urlretrieve(url, target_path)
    return target_path


def build_binary(release: bool) -> Path:
    cmd = ["cargo", "build", "-p", "slain-player"]
    if release:
        cmd.append("--release")
    subprocess.run(cmd, check=True)

    binary = Path("target") / ("release" if release else "debug") / "slain"
    if not binary.exists():
        raise RuntimeError(f"Expected binary not found at {binary}")
    return binary


def run_fixture(binary: Path, fixture: Dict[str, Any], file_path: Path, log_dir: Path) -> Dict[str, Any]:
    fixture_id = fixture["id"]
    frames = int(fixture.get("frames", 60))
    log_path = log_dir / f"{fixture_id}.log"

    cmd = [
        str(binary),
        "--headless",
        "--input",
        str(file_path),
        "--frames",
        str(frames),
    ]

    env = os.environ.copy()
    env.setdefault("RUST_LOG", "info")

    start = time.time()
    proc = subprocess.run(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
        text=True,
    )
    duration = time.time() - start

    log_dir.mkdir(parents=True, exist_ok=True)
    with log_path.open("w", encoding="utf-8") as handle:
        handle.write(f"fixture: {fixture_id}\n")
        handle.write(f"codec: {fixture.get('codec')}\n")
        handle.write(f"container: {fixture.get('container')}\n")
        handle.write(f"file: {file_path}\n")
        handle.write(f"frames: {frames}\n")
        handle.write(f"exit_code: {proc.returncode}\n")
        handle.write(f"duration_seconds: {duration:.2f}\n")
        handle.write("\n--- output ---\n")
        handle.write(proc.stdout)

    return {
        "id": fixture_id,
        "codec": fixture.get("codec"),
        "container": fixture.get("container"),
        "frames": frames,
        "result": "pass" if proc.returncode == 0 else "fail",
        "log": str(log_path),
        "duration": duration,
    }


def write_report(results: List[Dict[str, Any]], suite: str, report_path: Path) -> None:
    report_path.parent.mkdir(parents=True, exist_ok=True)

    platform_info = platform.platform()
    report_lines = [
        "# Headless Playback Report",
        "",
        f"- Suite: {suite}",
        f"- Platform: {platform_info}",
        f"- Generated: {time.strftime('%Y-%m-%d %H:%M:%S %Z')}",
        "",
        "| Fixture | Codec | Container | Frames | Result |",
        "| --- | --- | --- | --- | --- |",
    ]

    for result in results:
        report_lines.append(
            "| {id} | {codec} | {container} | {frames} | {result} |".format(
                **result
            )
        )

    report_lines.append("")
    report_lines.append("## Logs")
    for result in results:
        report_lines.append(f"- {result['id']}: {result['log']}")

    report_path.write_text("\n".join(report_lines), encoding="utf-8")


def main() -> int:
    args = parse_args()
    fixtures_path = Path(args.fixtures)
    fixtures = load_fixtures(fixtures_path)

    if args.suite == "smoke":
        fixtures = [f for f in fixtures if f.get("suite") == "smoke"]

    if not fixtures:
        raise RuntimeError("No fixtures selected for suite.")

    downloads_dir = Path("slain-player/fixtures/downloads")
    binary = build_binary(args.release) if not args.skip_build else None

    log_dir = Path(args.log_dir)
    results: List[Dict[str, Any]] = []

    for fixture in fixtures:
        file_path = ensure_download(fixture, downloads_dir)
        if binary is None:
            binary = Path("target") / ("release" if args.release else "debug") / "slain"
        results.append(run_fixture(binary, fixture, file_path, log_dir))

    report_path = Path(args.report)
    write_report(results, args.suite, report_path)

    failures = [result for result in results if result["result"] != "pass"]
    if failures:
        print("Headless playback failures detected:")
        for failure in failures:
            print(f"- {failure['id']}: {failure['log']}")
        return 1

    print(f"Headless playback completed successfully. Report: {report_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
