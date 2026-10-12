#!/usr/bin/env python3
"""Generate platform icons from the supplied PNG. Needs Python 3 and Pillow.

Runs on Windows, macOS and Linux, including .ico and .icns generation.
Outputs are committed; ordinary builds and packaging do not need Pillow.
"""

import base64
import io
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
DIR = ROOT / "assets" / "app-icon"
APP_ID = "ai.storyteller.pdfcraft"


def svg_image(png, size):
    """A self-contained SVG container, not a vector tracing of the supplied artwork."""
    encoded = base64.b64encode(png).decode("ascii")
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
        f'width="{size}" height="{size}" viewBox="0 0 {size} {size}">\n'
        '<title>PdfCraft PDF document emblem</title>\n'
        f'<image width="{size}" height="{size}" xlink:href="data:image/png;base64,{encoded}"/>\n'
        '</svg>\n'
    )


def main():
    source = DIR / "pdfcraft.png"
    with Image.open(source) as image:
        image = image.convert("RGBA")
    if image.width != image.height:
        raise ValueError("The master PNG must be square so the logo is never stretched.")

    def resized(size):
        return image.resize((size, size), Image.Resampling.LANCZOS)

    # The source already has transparent margins; preserve its framing on every OS.
    mac = resized(1024)
    mac.save(DIR / "pdfcraft-1024.png")
    mac.save(DIR / "pdfcraft.icns", format="ICNS")
    resized(256).save(
        DIR / "pdfcraft.ico", format="ICO",
        sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)],
    )
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        out = DIR / "hicolor" / f"{size}x{size}" / "apps" / f"{APP_ID}.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        resized(size).save(out)

    (DIR / "pdfcraft.svg").write_bytes(svg_image(source.read_bytes(), image.width).encode("utf-8"))
    # Resource 2 supplies Windows PDF file associations with the same contributor emblem.
    for extension in ("svg", "ico"):
        (DIR / f"pdfcraft-document.{extension}").write_bytes((DIR / f"pdfcraft.{extension}").read_bytes())
    for size, out in (
        (256, DIR / "pdfcraft-small.svg"),
        (512, DIR / "hicolor" / "scalable" / "apps" / f"{APP_ID}.svg"),
    ):
        png = io.BytesIO()
        resized(size).save(png, format="PNG")
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(svg_image(png.getvalue(), size).encode("utf-8"))
    print(f"Icons written to {DIR}")
    print("Update ATTRIBUTION.toml hashes, then run cargo xtask assets --write.")


if __name__ == "__main__":
    main()
