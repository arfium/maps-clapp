#!/usr/bin/env python3
"""assets/icon.png → src-tauri/icons/icon.ico, the icon Windows compiles into the exe.

Called by scripts/render-icon.sh; separate from it only because a heredoc inside a shell
script is a bad place to keep code that has to be read.

Seven sizes, because Windows picks one per context and scaling one down looks like it: 16
in a title bar, 32 on the taskbar, 48 in a list, 256 in Explorer's large view.
"""

import sys
from pathlib import Path

from PIL import Image

SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]

# Apple's Dock grid: the tile sits at ~80% of the canvas, centred on transparency. The
# same inset clappkit's dock-icon applies at runtime — generated HERE and committed, so
# packaging needs no tool, no sibling checkout and no ssh key (the silent fallback that
# once shipped a full-bleed icon is gone for good).
DOCK_FILL = 0.80


def make_dock(root: Path) -> None:
    src = root / "assets" / "icon.png"
    out = root / "assets" / "icon-dock.png"
    im = Image.open(src).convert("RGBA")
    side = max(im.size)
    target = round(side * DOCK_FILL)
    scaled = im.resize((target, target), Image.LANCZOS)
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    off = (side - target) // 2
    canvas.paste(scaled, (off, off), scaled)
    canvas.save(out)
    print(f"assets/icon-dock.png — {DOCK_FILL:.0%} inset for the Dock/.icns")


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    src = root / "assets" / "icon.png"
    out = root / "src-tauri" / "icons" / "icon.ico"
    if not src.is_file():
        print(f"{src} is missing — run scripts/render-icon.sh first", file=sys.stderr)
        return 1
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.open(src).convert("RGBA").save(out, format="ICO", sizes=SIZES)
    print(f"src-tauri/icons/icon.ico — {', '.join(str(w) for w, _ in SIZES)}")
    make_dock(root)
    return 0


if __name__ == "__main__":
    sys.exit(main())
