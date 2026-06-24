"""Generate Flowntier Tauri app icons via MiniMax image-01 API.

Outputs:
  apps/desktop/src-tauri/icons/icon-1024.png  (Tauri primary)
  apps/desktop/src-tauri/icons/icon-256.png
  apps/desktop/src-tauri/icons/icon-128.png
  apps/desktop/src-tauri/icons/icon.ico       (Tauri Windows resource)
"""
from __future__ import annotations

import base64
import io
import os
import struct
import sys
from pathlib import Path

import httpx

API = "https://api.minimaxi.com/v1/image_generation"
ICON_DIR = Path("apps/desktop/src-tauri/icons")
ICON_DIR.mkdir(parents=True, exist_ok=True)

# Three prompts with ascending detail so we get clean, on-brand results.
PROMPTS = [
    # 1024 — primary
    """Minimal geometric app icon for Flowntier, a dark-themed \
AI software-company dashboard. Center: a stylized letter 'A' formed by two \
overlapping triangles — one cyan #06b6d4, one violet #8b5cf6 — that together \
suggest a folder/eye/lens (the observer watching the workers). Background: \
deep blue-gray #0f172a with a subtle radial glow. Style: modern, flat, \
geometric, like Linear or Vercel app icons. No text. No border ring. \
Square 1:1 aspect ratio.""",
]


def gen(prompt: str, n: int = 2) -> list[bytes]:
    """Call MiniMax image-01 API and return PNG bytes."""
    api_key = os.environ["MINIMAX_API_KEY"]
    headers = {"Authorization": f"Bearer {api_key}"}
    body = {
        "model": "image-01",
        "prompt": prompt,
        "n": n,
        "width": 1024,
        "height": 1024,
    }
    with httpx.Client(timeout=120.0) as client:
        r = client.post(API, headers=headers, json=body)
        r.raise_for_status()
        data = r.json()
    raw = data["data"]
    urls: list[str] = []
    if isinstance(raw, list):
        if raw and isinstance(raw[0], dict):
            urls = [d["url"] for d in raw if d.get("url")]
            if not urls:
                urls = raw[0].get("image_urls", []) or []
        elif raw and isinstance(raw[0], str):
            urls = list(raw)
    elif isinstance(raw, dict):
        urls = raw.get("image_urls") or raw.get("urls") or []
    if not urls:
        print("no image URLs in response:", data, file=sys.stderr)
        sys.exit(1)
    print(f"got {len(urls)} URLs; downloading first")
    out: list[bytes] = []
    for url in urls[:n]:
        with httpx.Client(timeout=60.0) as client:
            img_r = client.get(url)
            img_r.raise_for_status()
            out.append(img_r.content)
    return out


def png_to_ico(png_bytes: bytes, sizes: tuple[int, ...] = (32, 64, 128, 256)) -> bytes:
    """Build a Windows ICO file embedding the given PNG at multiple sizes.

    ICO can wrap PNGs directly (Vista+). We embed the source PNG at
    each requested size; the ICO header lists them. Windows picks the
    best size for the target DPI.
    """
    # Resize PNGs via in-memory Pillow if available; else embed the
    # original PNG at every listed size (Windows will scale).
    try:
        from PIL import Image
        have_pil = True
    except ImportError:
        have_pil = False

    images: list[tuple[int, int, bytes]] = []
    if have_pil:
        src = Image.open(io.BytesIO(png_bytes))
        for size in sizes:
            im = src.resize((size, size), Image.LANCZOS)
            buf = io.BytesIO()
            im.save(buf, format="PNG")
            images.append((size, size, buf.getvalue()))
    else:
        # Fallback: embed the same PNG for each entry
        for size in sizes:
            images.append((size, size, png_bytes))

    # ICONDIR header
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    payloads = b""
    for w, h, data in images:
        bw = 0 if w >= 256 else w
        bh = 0 if h >= 256 else h
        entries += struct.pack(
            "<BBBBHHII",
            bw, bh, 0, 0, 1, 32,
            len(data), offset,
        )
        payloads += data
        offset += len(data)
    return out + entries + payloads


def main() -> int:
    primary_prompt = PROMPTS[0]
    images = gen(primary_prompt, n=2)
    # Pick the first (usually best quality / most on-brand)
    primary = images[0]
    out_1024 = ICON_DIR / "icon-1024.png"
    out_1024.write_bytes(primary)
    print(f"wrote {out_1024} ({len(primary)} bytes)")

    # Save alternates too in case the user wants to pick
    for i, img in enumerate(images[1:], start=2):
        alt_path = ICON_DIR / f"icon-1024-alt{i-1}.png"
        alt_path.write_bytes(img)
        print(f"wrote {alt_path} ({len(img)} bytes)")

    # Down-scale via PIL
    try:
        from PIL import Image
        src = Image.open(io.BytesIO(primary))
        for size in (256, 128, 64, 32):
            im = src.resize((size, size), Image.LANCZOS)
            out = ICON_DIR / f"icon-{size}.png"
            buf = io.BytesIO()
            im.save(buf, format="PNG")
            out.write_bytes(buf.getvalue())
            print(f"wrote {out} ({buf.tell()} bytes)")
    except ImportError:
        print("PIL not available; skipping down-scales", file=sys.stderr)

    # Build the ICO (multi-size PNG)
    ico_bytes = png_to_ico(primary)
    out_ico = ICON_DIR / "icon.ico"
    out_ico.write_bytes(ico_bytes)
    print(f"wrote {out_ico} ({len(ico_bytes)} bytes)")

    return 0


if __name__ == "__main__":
    sys.exit(main())