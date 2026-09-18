// Generates a small disposable project for tests/browser/e2e.mjs:
//   node tests/browser/fixture.mjs <empty-directory>
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { crc32, deflateSync } from 'node:zlib';

const root = process.argv[2];
if (!root) throw new Error('usage: fixture.mjs <directory>');

function png(width, height, paint) {
  const chunk = (kind, data) => {
    const body = Buffer.concat([Buffer.from(kind), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0); body.copy(out, 4); out.writeUInt32BE(crc32(body), body.length + 4);
    return out;
  };
  const rows = Buffer.alloc(height * (width * 4 + 1));
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) rows.set(paint(x, y), y * (width * 4 + 1) + 1 + x * 4);
  const header = Buffer.alloc(13); header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header.set([8, 6, 0, 0, 0], 8);
  // Level 0 leaves plenty for a lossless optimizer to find.
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', header), chunk('IDAT', deflateSync(rows, { level: 0 })), chunk('IEND', Buffer.alloc(0))]);
}
function write(path, data) { const file = join(root, path); mkdirSync(dirname(file), { recursive: true }); writeFileSync(file, data); }

// Deterministic noise: lossy codecs score poorly on it, which yields warnings.
let seed = 7; const random = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
const gradient = (x, y) => [x * 2 % 256, y * 2 % 256, 140, 255];
const noisy = () => [random() * 255, random() * 255, random() * 255, 255];
const badge = (x, y) => { const inside = (x - 48) ** 2 + (y - 48) ** 2 < 1600; return [230, 60 + x, 60, inside ? 255 : 0]; };

for (let i = 0; i < 6; i++) write(`App/Resources/gradient-${i}.png`, png(96 + i * 8, 96, gradient));
for (let i = 0; i < 4; i++) write(`App/Resources/noise-${i}.png`, png(128, 128, noisy));
write('App/Resources/badge.png', png(96, 96, badge));
write('App/Other/badge-copy.png', png(96, 96, badge));
write('App/Resources/intro.mp3', Buffer.from('ID3 placeholder audio'));
write('App/Assets.xcassets/Contents.json', '{"info":{"author":"xcode","version":1}}');
write('App/Assets.xcassets/Hero.imageset/Contents.json', JSON.stringify({ images: [{ filename: 'hero@2x.png', idiom: 'universal', scale: '2x' }, { filename: 'hero@3x.png', idiom: 'universal', scale: '3x' }], info: { author: 'xcode', version: 1 } }));
write('App/Assets.xcassets/Hero.imageset/hero@2x.png', png(120, 80, gradient));
write('App/Assets.xcassets/Hero.imageset/hero@3x.png', png(180, 120, gradient));
write('App/View.swift', 'let image = UIImage(named: "Hero")\n');
