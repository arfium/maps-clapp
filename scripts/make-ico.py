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
    return 0


if __name__ == "__main__":
    sys.exit(main())
