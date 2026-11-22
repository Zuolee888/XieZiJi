"""Render a local ordinary font for optional AutoTrace centerline conversion."""
import sys
import json
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

font_path, text_path, output, size, dpi = sys.argv[1:]
requested_dpi = float(dpi)


def layout(render_dpi):
    font = ImageFont.truetype(font_path, max(1, round(float(size) * render_dpi / 25.4)))
    probe = ImageDraw.Draw(Image.new('L', (1, 1), 255))
    box = probe.multiline_textbbox((0, 0), text, font=font)
    return font, box, box[2] - box[0] + 8, box[3] - box[1] + 8


text = open(text_path, encoding='utf-8').read()
font, box, width, height = layout(requested_dpi)
if width * height > 50_000_000:
    # 尺寸由字号与 dpi 决定：像素数随 dpi 平方增长。这段文字是我们自己渲染的，
    # 降密度只是取样变疏，不影响字形位置，所以直接按上限反推、继续渲染。
    # 实际用了多少 dpi 要报回去，否则调用方仍按请求值换算像素→毫米，
    # 整段文字会被放大。
    dpi = requested_dpi * (50_000_000 / (width * height)) ** 0.5
    font, box, width, height = layout(dpi)
    if width * height > 50_000_000:
        raise ValueError(
            '这段文字即使把栅格分辨率降到 {:.0f} dpi 也需要 {:.0f} 万像素，超过 5000 万的上限。'
            '请缩小字号或减少文字后重试。'.format(dpi, width * height / 10000)
        )
image = Image.new('L', (width, height), 255)
ImageDraw.Draw(image).multiline_text((4-box[0], 4-box[1]), text, font=font, fill=0)
image.save(output)
Path(output).with_suffix('.json').write_text(json.dumps({
    'origin_x': 4-box[0],
    'baseline_y': 4-box[1]+font.getmetrics()[0],
    'advance': max(font.getlength(line) for line in text.split('\n')),
    'dpi': float(dpi),
}), encoding='utf-8')
