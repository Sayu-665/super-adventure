// Rasterises ../assets/logo.svg into the icons used by the app and by electron-builder:
//   build/icon.png (1024x1024), build/icon.ico (256/128/64/48/32/16) and src/assets/icon.png (512x512, window icon).
// Dev-only helper: renders the SVG with Playwright's Chromium (run: node scripts/generate-icons.mjs).
import { chromium } from 'playwright';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const svg = fs.readFileSync(path.join(root, '..', 'assets', 'logo.svg'), 'utf8');

function findChromium() {
  const base = process.env.PLAYWRIGHT_BROWSERS_PATH || '/opt/pw-browsers';
  const preferred = chromium.executablePath();
  if (fs.existsSync(preferred)) return preferred;
  for (const dir of fs.existsSync(base) ? fs.readdirSync(base) : []) {
    if (!dir.startsWith('chromium-')) continue;
    for (const sub of ['chrome-linux64/chrome', 'chrome-linux/chrome']) {
      const candidate = path.join(base, dir, sub);
      if (fs.existsSync(candidate)) return candidate;
    }
  }
  return undefined; // let Playwright decide
}

async function render(page, size) {
  await page.setViewportSize({ width: size, height: size });
  const html = `<!doctype html><html><head><style>html,body{margin:0;background:transparent}
    svg{display:block;width:${size}px;height:${size}px}</style></head><body>${svg}</body></html>`;
  await page.setContent(html);
  return page.screenshot({ omitBackground: true, type: 'png' });
}

// ICO container with PNG-compressed images (supported since Windows Vista).
function buildIco(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  const entries = [];
  let offset = 6 + 16 * images.length;
  for (const { size, data } of images) {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size >= 256 ? 0 : size, 0);
    entry.writeUInt8(size >= 256 ? 0 : size, 1);
    entry.writeUInt8(0, 2); // palette
    entry.writeUInt8(0, 3); // reserved
    entry.writeUInt16LE(1, 4); // colour planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(data.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += data.length;
    entries.push(entry);
  }
  return Buffer.concat([header, ...entries, ...images.map((i) => i.data)]);
}

const browser = await chromium.launch({ executablePath: findChromium(), args: ['--no-sandbox'] });
try {
  const page = await browser.newPage({ deviceScaleFactor: 1 });
  fs.writeFileSync(path.join(root, 'build', 'icon.png'), await render(page, 1024));
  fs.writeFileSync(path.join(root, 'src', 'assets', 'icon.png'), await render(page, 512));
  const icoImages = [];
  for (const size of [256, 128, 64, 48, 32, 16]) icoImages.push({ size, data: await render(page, size) });
  fs.writeFileSync(path.join(root, 'build', 'icon.ico'), buildIco(icoImages));
  fs.copyFileSync(path.join(root, '..', 'assets', 'logo.svg'), path.join(root, 'src', 'assets', 'logo.svg'));
  console.log('Icons written to build/ and src/assets/');
} finally {
  await browser.close();
}
