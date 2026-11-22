// 把「写字机工程」里的两个模板工程整理成随发布包分发的样板：
//   desktop/assets/samples/中原工学院稿纸模板工程.inkproj  —— 稿纸样板工程
//   desktop/assets/samples/自学院稿纸.json                —— 稿纸配套模板
//   desktop/assets/samples/自物理实验报告.json            —— 物理实验配套模板
//
// 为什么要重新打包而不是直接拷：`中原工学院稿纸模板工程.inkproj` 里的
// config/conversion.json 仍是 dpi 300，而当前版本的默认栅格分辨率是 450。
// 打开工程会用工程自带的值覆盖本机设置，所以直接拷过去等于把旧的 300 又发出去。
//
// 用法：node scripts/prepare-samples.mjs <写字机工程目录>
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { crc32, deflateRawSync, inflateRawSync } from "node:zlib";

const desktop = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sourceDir = process.argv[2];
if (!sourceDir) {
  console.error("用法：node scripts/prepare-samples.mjs <写字机工程目录>");
  process.exit(1);
}
const outDir = join(desktop, "assets", "samples");
await mkdir(outDir, { recursive: true });

// --- 最小 zip 读写。只处理「stored」与「deflate」两种方法，够 .inkproj 用。
function readZip(buffer) {
  const eocd = buffer.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (eocd < 0) throw new Error("不是 zip：找不到 EOCD");
  const count = buffer.readUInt16LE(eocd + 10);
  let offset = buffer.readUInt32LE(eocd + 16);
  const entries = [];
  for (let i = 0; i < count; i++) {
    if (buffer.readUInt32LE(offset) !== 0x02014b50) throw new Error("中央目录损坏");
    const method = buffer.readUInt16LE(offset + 10);
    const compressedSize = buffer.readUInt32LE(offset + 20);
    const nameLength = buffer.readUInt16LE(offset + 28);
    const extraLength = buffer.readUInt16LE(offset + 30);
    const commentLength = buffer.readUInt16LE(offset + 32);
    const localOffset = buffer.readUInt32LE(offset + 42);
    const name = buffer.toString("utf8", offset + 46, offset + 46 + nameLength);
    const localNameLength = buffer.readUInt16LE(localOffset + 26);
    const localExtraLength = buffer.readUInt16LE(localOffset + 28);
    const start = localOffset + 30 + localNameLength + localExtraLength;
    const raw = buffer.subarray(start, start + compressedSize);
    entries.push({ name, method, data: method === 0 ? Buffer.from(raw) : inflateRawSync(raw) });
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}

function writeZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const { name, data } of entries) {
    const nameBytes = Buffer.from(name, "utf8");
    const deflated = deflateRawSync(data);
    const useDeflate = deflated.length < data.length;
    const payload = useDeflate ? deflated : data;
    const method = useDeflate ? 8 : 0;
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6); // UTF-8 文件名
    local.writeUInt16LE(method, 8);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(payload.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, payload);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(method, 10);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(payload.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt32LE(offset, 42);
    centrals.push(central, nameBytes);
    offset += local.length + nameBytes.length + payload.length;
  }
  const centralSize = centrals.reduce((sum, part) => sum + part.length, 0);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(centralSize, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, ...centrals, end]);
}

// --- 稿纸工程：只把 conversion.dpi 拉到当前默认值 450，其余字节原样保留。
const gridSource = join(sourceDir, "中原工学院稿纸模板工程.inkproj");
const gridEntries = readZip(await readFile(gridSource));
const conversion = gridEntries.find((e) => e.name === "config/conversion.json");
if (!conversion) throw new Error("稿纸工程里没有 config/conversion.json");
const before = JSON.parse(conversion.data.toString("utf8"));
conversion.data = Buffer.from(JSON.stringify({ ...before, dpi: 450 }), "utf8");
const gridOut = join(outDir, "中原工学院稿纸模板工程.inkproj");
await writeFile(gridOut, writeZip(gridEntries));
console.log(`稿纸工程 dpi ${before.dpi} -> 450，写出 ${gridOut}`);

// --- 两个配套模板：id 与 templates.rs 里的内置项保持一致。
// 用户把这两个文件拖进 %LOCALAPPDATA%\OpenInkStudio\templates 时，list() 会因
// id 以 builtin- 开头而跳过 —— 这是有意的：内置项已经在列表里，再收一份就只剩
// 两个同名模板让人选错。它们随包分发的用途是「可读、可对比、可改」，不是安装。
for (const [file, id] of [
  ["自学院稿纸.json", "builtin-grid"],
  ["自物理实验报告.json", "builtin-physics"],
]) {
  const template = JSON.parse(await readFile(join(sourceDir, file), "utf8"));
  template.id = id;
  const target = join(outDir, file);
  await writeFile(target, JSON.stringify(template, null, 2) + "\n");
  console.log(`${file} -> id=${id}`);
}
