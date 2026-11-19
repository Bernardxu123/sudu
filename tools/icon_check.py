from PIL import Image

im = Image.open("assets/icon-256.png").convert("RGBA")
print(im.getpixel((0, 0)), im.getpixel((5, 5)), im.getpixel((128, 128)))
bg = Image.new("RGBA", im.size, (32, 32, 32, 255))
bg.alpha_composite(im)
bg.save("assets/icon-preview.png")
