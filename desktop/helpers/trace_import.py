"""Visible external raster pipeline: PyMuPDF/Pillow -> AutoTrace (no machine IO)."""
import argparse
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET

# Optional direct-CLI development discovery. Rust additionally passes explicit
# module directories through PYTHONPATH, independently of helper location/CWD.
for parent in Path(__file__).resolve().parents:
    cached = parent / '.cache' / 'pdf-review'
    if cached.is_dir():
        sys.path.append(str(cached))


def trace_image(image, output, cmd):
    from PIL import ImageOps

    ppm = Path(output).with_suffix('.ppm')
    image.save(ppm)
    run_args = dict(check=True, capture_output=True, text=True,
                    creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    try:
        subprocess.run(cmd, **run_args)
    except subprocess.CalledProcessError as error:
        if error.returncode & 0xffffffff != 0xc0000005:
            raise
        # Some Windows AutoTrace inputs crash without a white guard border.
        ImageOps.expand(image, border=2, fill='white').save(ppm)
        try:
            subprocess.run(cmd, **run_args)
        except subprocess.CalledProcessError as retry:
            raise RuntimeError(
                f'AutoTrace crashed (0xC0000005); border retry also failed '
                f'(exit {retry.returncode}). Original image was not modified. '
                f'Try a cropped image or another AutoTrace build. {retry.stderr or ""}'
            ) from retry
        tree = ET.parse(output)
        root = tree.getroot()
        root.set('width', str(image.width))
        root.set('height', str(image.height))
        # Undo only the guard border; keep the original pixel-to-mm mapping.
        root.set('viewBox', f'2 2 {image.width} {image.height}')
        tree.write(output, encoding='utf-8', xml_declaration=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('source')
    p.add_argument('output')
    p.add_argument('--autotrace', required=True)
    p.add_argument('--page', type=int, default=1)
    p.add_argument('--dpi', type=float, default=300)
    p.add_argument('--threshold', type=int, default=180)
    p.add_argument('--conservative', action='store_true')
    p.add_argument('--filter-iterations', type=int, default=4)
    p.add_argument('--error-threshold', type=float, default=2.0)
    p.add_argument('--line-threshold', type=float, default=1.0)
    p.add_argument('--despeckle-level', type=int, default=0)
    a = p.parse_args()
    try:
        from PIL import Image
    except ImportError:
        raise RuntimeError('Missing Pillow. Install: python -m pip install Pillow PyMuPDF')
    if not 36 <= a.dpi <= 1200 or not float(a.dpi).is_integer() or not 0 <= a.threshold <= 255:
        raise ValueError('DPI must be an integer 36..1200; threshold must be 0..255')
    if not 0 <= a.filter_iterations <= 20 or not 0 <= a.despeckle_level <= 20 or not 0 < a.error_threshold <= 20 or not 0 < a.line_threshold <= 20:
        raise ValueError('AutoTrace filter/despeckle must be 0..20; error/line thresholds must be >0..20')
    if Path(a.source).suffix.lower() == '.pdf':
        try:
            import fitz
        except ImportError:
            raise RuntimeError('Missing PyMuPDF. Install: python -m pip install Pillow PyMuPDF')
        with fitz.open(a.source) as pdf:
            if not 1 <= a.page <= len(pdf):
                raise ValueError(f'PDF page must be 1..{len(pdf)}')
            page = pdf[a.page - 1]
            # 页面尺寸以毫米算，栅格分辨率只影响取样密度。大画幅的板子（比如 381x508 mm
            # 的实验报告纸）在 450 dpi 下就是六千万像素，超过上限。与其让用户自己回去改
            # 分辨率，不如按上限反推一个还够用的 dpi 继续画 —— 纸越大，取样越疏，
            # 但笔画的位置不变。实际用了多少 dpi 必须报回去（EFFECTIVE_DPI），
            # 否则 Rust 侧仍按 450 换算像素→毫米，整页会缩小 9%。
            if page.rect.width * page.rect.height * (a.dpi / 72) ** 2 > 50_000_000:
                a.dpi = int((50_000_000 / (page.rect.width * page.rect.height)) ** 0.5 * 72)
                if a.dpi < 36:
                    raise ValueError(
                        '这一页的纸张尺寸是 {:.0f}x{:.0f} 毫米，即使降到 36 dpi 也需要超过 '
                        '5000 万像素，无法转换。请拆分页面后再试。'.format(
                            page.rect.width * 25.4 / 72,
                            page.rect.height * 25.4 / 72,
                        )
                    )
            pix = page.get_pixmap(dpi=round(a.dpi), colorspace=fitz.csRGB, alpha=False)
            image = Image.frombytes('RGB', (pix.width, pix.height), pix.samples)
    else:
        image = Image.open(a.source)
        image.seek(a.page - 1)
        image = image.convert('RGBA')
        white = Image.new('RGBA', image.size, 'white')
        image = Image.alpha_composite(white, image).convert('RGB')
    if image.width * image.height > 50_000_000:
        # 位图的像素是既成事实，不能像 PDF 那样重采样降级（换密度等于改用户的图）。
        # 只能让他自己缩图，所以要说清当前多大、要缩到多少。
        scale = (50_000_000 / (image.width * image.height)) ** 0.5
        raise ValueError(
            '这张图的像素是 {}x{}，共 {:.0f} 万，超过 5000 万的上限。'
            '把它缩到 {}x{} 或更小后再导入。'.format(
                image.width, image.height, image.width * image.height / 10000,
                int(image.width * scale), int(image.height * scale),
            )
        )
    # Threshold only: no enlargement, morphological cleanup or invented strokes.
    image = image.convert('L').point(lambda v: 0 if v < a.threshold else 255).convert('RGB')
    ppm = Path(a.output).with_suffix('.ppm')
    if a.conservative:
        a.filter_iterations, a.error_threshold, a.line_threshold, a.despeckle_level = 1, 0.5, 0.5, 0
    cmd = [a.autotrace, '-centerline', '-background-color', 'FFFFFF',
           '-despeckle-level', str(a.despeckle_level),
           '-filter-iterations', str(a.filter_iterations),
           '-error-threshold', str(a.error_threshold), '-line-threshold', str(a.line_threshold)]
    cmd += ['-output-file', a.output, str(ppm)]
    trace_image(image, a.output, cmd)
    # 实际取样密度可能与请求值不同（超大画幅被上限压低）。Rust 侧拿这个数换算
    # 像素→毫米，读不到就退回请求值，所以必须打在 stdout 上。
    print('EFFECTIVE_DPI', a.dpi)


if __name__ == '__main__':
    try:
        main()
    except Exception as e:
        # 用户看到的应该是「怎么办」，不是模块搜索路径。只有真的缺依赖（ImportError /
        # 找不到脚本）时，Python 路径与 sys.path 才有诊断价值；其余情况（dpi 超限、
        # 参数非法）把这几行噪声收起来，免得挡住第一句可执行的提示。
        if isinstance(e, (ImportError, RuntimeError)):
            print(f'{e}\nPython: {sys.executable}\nHelper: {Path(__file__).resolve()}\nModule search: {sys.path}', file=sys.stderr)
        else:
            print(str(e), file=sys.stderr)
        sys.exit(1)
