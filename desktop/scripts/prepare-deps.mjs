// 把 PDF/图片转换要用到的三份外部依赖收拢到 desktop/ 下，供 tauri bundle 打进安装包。
//
// 为什么要有这一步：0.7.0 的安装包只有 3.5 MB，资源清单里只写了 helpers/*，
// 于是 PDF/PNG/JPEG 中心线、字体中心线、手写识别、纸张背景处理这四类功能在
// 新装的电脑上全都报「找不到 AutoTrace / Python」。依赖本身一直存在于构建机的
// .cache 与 %LOCALAPPDATA% 里，只是从来没有被放进包里。
//
// 这个脚本是幂等的：目标目录已经齐全就跳过，不重复拷 600 MB。
// 用法：node scripts/prepare-deps.mjs [--force] [--require]
//   默认只尽力而为（缺依赖时警告并继续），免得没有 .cache 的人连 build 都跑不起来；
//   发版前加 --require，缺依赖即失败，不会静默发出一个缺功能的安装包。
import { cp, mkdir, readdir, stat } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = resolve(desktop, "..");
const force = process.argv.includes("--force");
const require_ = process.argv.includes("--require");

// 每一项：装进 desktop/<name>，从 candidates 里第一个存在的目录拷。
// marker 是「有它才算齐」的标志文件，避免半途中断后误判为已完成。
const deps = [
  {
    name: "autotrace",
    marker: "autotrace.exe",
    candidates: [
      join(repo, ".cache", "autotrace", "bin-2020"),
      join(repo, "dist", "openink-studio-portable", "autotrace"),
    ],
  },
  {
    name: "python",
    marker: "python.exe",
    candidates: [
      join(repo, ".cache", "desktop-python", "Scripts"),
      join(repo, "dist", "openink-studio-portable", "python"),
    ],
  },
  {
    name: "ocr-models",
    marker: "ch_PP-OCRv5_det_server.onnx",
    candidates: [
      join(process.env.LOCALAPPDATA ?? "", "OpenInkStudio", "ocr-models"),
      join(repo, "dist", "openink-studio-portable", "ocr-models"),
    ],
  },
];

function firstExisting(candidates) {
  for (const candidate of candidates) {
    if (candidate && existsSync(candidate)) return candidate;
  }
  return null;
}

async function count(dir) {
  let files = 0;
  let bytes = 0;
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      const nested = await count(full);
      files += nested.files;
      bytes += nested.bytes;
    } else if (entry.isFile()) {
      files += 1;
      bytes += (await stat(full)).size;
    }
  }
  return { files, bytes };
}

let missing = [];
for (const { name, marker, candidates } of deps) {
  const source = firstExisting(candidates);
  if (!source) {
    missing.push(`  ${name}: 找不到来源，试过 ${candidates.filter(Boolean).join(" / ")}`);
    continue;
  }
  const target = join(desktop, name);
  if (!force && existsSync(join(target, marker))) {
    const { files, bytes } = await count(target);
    console.log(`skip ${name}: 已有 ${files} 个文件（${(bytes / 1048576).toFixed(1)} MB）`);
    continue;
  }
  await mkdir(target, { recursive: true });
  // Windows 上 Python 目录里可能有符号链接/只读文件，保留原样复制。
  await cp(source, target, { recursive: true, force: true, verbatimSymlinks: true });
  const { files, bytes } = await count(target);
  console.log(`copied ${name}: ${source} -> ${target}（${files} 个文件，${(bytes / 1048576).toFixed(1)} MB）`);
}

if (missing.length) {
  const report = [
    "缺少随程序分发的依赖，这个安装包会缺少 PDF / 图片转换能力：",
    ...missing,
    "",
    "把对应的目录放到上面任意一个来源位置，或用 --force 重跑。",
  ];
  if (require_) {
    console.error([...report, "", "（本次带 --require，按发版要求中止。）"].join("\n"));
    process.exit(1);
  }
  console.warn([...report, "", "（本次未带 --require，继续构建；仅用于本地试编译。）"].join("\n"));
}
