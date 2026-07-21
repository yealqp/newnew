// scripts/gen-desktop-icons.js
// 读取 desktop/src-tauri/icons/icon.svg，光栅化为 Tauri 所需尺寸：
//   icon.png(512), 128x128.png, 32x32.png；并合成 icon.ico(多尺寸)。
// 保留原文件名以免改 tauri.conf.json 的 bundle.icon 列表。
const fs = require('fs');
const path = require('path');
const { Resvg } = require('@resvg/resvg-js');

const svgDir = path.resolve(__dirname, '..', 'desktop', 'src-tauri', 'icons');
const svgPath = path.join(svgDir, 'icon.svg');
const svgBuffer = fs.readFileSync(svgPath);

function renderPng(size) {
  const resvg = new Resvg(svgBuffer, {
    fitTo: { mode: 'width', value: size },
    background: 'transparent',
  });
  return resvg.render().asPng();
}

// PNG: 512 / 128 / 32
fs.writeFileSync(path.join(svgDir, 'icon.png'), renderPng(512));
fs.writeFileSync(path.join(svgDir, '128x128.png'), renderPng(128));
fs.writeFileSync(path.join(svgDir, '32x32.png'), renderPng(32));

// ICO: ICONDIR + ICONDIRENTRY[] + 多份 PNG 数据（现代 ICO 接受内嵌 PNG）。
function buildIco(sizes) {
  const pngs = sizes.map((s) => ({ size: s, data: renderPng(s) }));
  const headerSize = 6;
  const dirEntrySize = 16;
  const dirSize = headerSize + dirEntrySize * pngs.length;
  const header = Buffer.alloc(headerSize);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type = ICO
  header.writeUInt16LE(pngs.length, 4);

  const dir = Buffer.alloc(dirEntrySize * pngs.length);
  let dataOffset = dirSize;
  let p = 0;
  for (const { size, data } of pngs) {
    const w = size >= 256 ? 0 : size; // 256 在目录项里写 0
    dir.writeUInt8(w, p + 0);
    dir.writeUInt8(w, p + 1);
    dir.writeUInt8(0, p + 2); // 0 色
    dir.writeUInt8(0, p + 3); // reserved
    dir.writeUInt16LE(1, p + 4); // planes
    dir.writeUInt16LE(32, p + 6); // bpp
    dir.writeUInt32LE(data.length, p + 8);
    dir.writeUInt32LE(dataOffset, p + 12);
    dataOffset += data.length;
    p += dirEntrySize;
  }
  return Buffer.concat([header, dir, ...pngs.map((x) => x.data)]);
}

fs.writeFileSync(path.join(svgDir, 'icon.ico'), buildIco([16, 32, 48, 64, 128, 256]));

console.log('[OK] 桌面端图标已生成（icon.png / 128x128.png / 32x32.png / icon.ico）');