import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

src_path = Path(sys.argv[1])
out_dir = Path(sys.argv[2])
out_dir.mkdir(parents=True, exist_ok=True)

src = Image.open(src_path).convert("RGB")
diff = ImageChops.difference(src, Image.new("RGB", src.size, (255, 255, 255)))
mask = diff.convert("L").point(lambda value: 255 if value > 16 else 0)
bbox = mask.getbbox() or (0, 0, src.width, src.height)

left, top, right, bottom = bbox
side = max(right - left, bottom - top)
pad = int(side * 0.12)
side += pad * 2
center_x = (left + right) // 2
center_y = (top + bottom) // 2

canvas = Image.new("RGB", (side, side), (255, 255, 255))
canvas.paste(src, (side // 2 - center_x, side // 2 - center_y))
art = canvas.resize((800, 800), Image.LANCZOS)

tile = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
draw = ImageDraw.Draw(tile)
draw.rounded_rectangle((0, 0, 1023, 1023), radius=230, fill=(255, 255, 255, 255))
tile.paste(art, (112, 112))

tile.save(out_dir / "icon-1024.png")
tile.resize((256, 256), Image.LANCZOS).save(out_dir / "icon-256.png")
tile.save(
    out_dir / "icon.ico",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)
print(f"bbox={bbox} tile={out_dir}")
