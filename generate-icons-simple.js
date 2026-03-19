import { writeFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import { deflateSync } from 'zlib';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const sizes = [
  { size: 32, name: '32x32.png' },
  { size: 128, name: '128x128.png' },
  { size: 256, name: '128x128@2x.png' },
  { size: 512, name: '512x512.png' }
];

const iconsDir = join(__dirname, 'src-tauri', 'icons');

// PNG signature
const PNG_SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);

// CRC32 implementation
function crc32(buf) {
  let crc = -1;
  for (let i = 0; i < buf.length; i++) {
    crc ^= buf[i];
    for (let j = 0; j < 8; j++) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xEDB88320 : 0);
    }
  }
  return (crc ^ -1) >>> 0;
}

function createChunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);

  const chunkData = Buffer.concat([Buffer.from(type), data]);
  const checksum = Buffer.alloc(4);
  checksum.writeUInt32BE(crc32(chunkData), 0);

  return Buffer.concat([
    length,
    chunkData,
    checksum
  ]);
}

function blendPixel(imageData, width, x, y, rgba) {
  if (x < 0 || y < 0 || x >= width) {
    return;
  }

  const idx = (y * width + x) * 4;
  const srcA = rgba[3] / 255;
  const dstA = imageData[idx + 3] / 255;
  const outA = srcA + dstA * (1 - srcA);

  if (outA <= 0) {
    return;
  }

  imageData[idx] = Math.round((rgba[0] * srcA + imageData[idx] * dstA * (1 - srcA)) / outA);
  imageData[idx + 1] = Math.round(
    (rgba[1] * srcA + imageData[idx + 1] * dstA * (1 - srcA)) / outA,
  );
  imageData[idx + 2] = Math.round(
    (rgba[2] * srcA + imageData[idx + 2] * dstA * (1 - srcA)) / outA,
  );
  imageData[idx + 3] = Math.round(outA * 255);
}

function pointSegmentDistance(px, py, x1, y1, x2, y2) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const lengthSq = dx * dx + dy * dy;

  if (lengthSq === 0) {
    return Math.hypot(px - x1, py - y1);
  }

  const t = Math.max(0, Math.min(1, ((px - x1) * dx + (py - y1) * dy) / lengthSq));
  const projX = x1 + t * dx;
  const projY = y1 + t * dy;
  return Math.hypot(px - projX, py - projY);
}

function drawRoundedRect(imageData, width, height, rect, radius, color) {
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const px = x + 0.5;
      const py = y + 0.5;

      const nearestX = Math.max(rect.x + radius, Math.min(px, rect.x + rect.w - radius));
      const nearestY = Math.max(rect.y + radius, Math.min(py, rect.y, rect.y + rect.h - radius));
      const dx = px - nearestX;
      const dy = py - nearestY;
      const distance = Math.hypot(dx, dy);

      if (
        (px >= rect.x + radius && px <= rect.x + rect.w - radius && py >= rect.y && py <= rect.y + rect.h) ||
        (py >= rect.y + radius && py <= rect.y + rect.h - radius && px >= rect.x && px <= rect.x + rect.w) ||
        distance <= radius
      ) {
        blendPixel(imageData, width, x, y, color);
      }
    }
  }
}

function drawStroke(imageData, width, height, points, strokeWidth, color) {
  const half = strokeWidth / 2;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const px = x + 0.5;
      const py = y + 0.5;

      for (let i = 0; i < points.length - 1; i++) {
        const [x1, y1] = points[i];
        const [x2, y2] = points[i + 1];
        if (pointSegmentDistance(px, py, x1, y1, x2, y2) <= half) {
          blendPixel(imageData, width, x, y, color);
          break;
        }
      }
    }
  }
}

function createMinimalPNG(width, height) {
  // IHDR chunk
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;  // bit depth
  ihdr[9] = 6;  // color type (RGBA)
  ihdr[10] = 0; // compression
  ihdr[11] = 0; // filter
  ihdr[12] = 0; // interlace

  // Create image data for the flat orange WLTP icon
  const bytesPerPixel = 4;
  const imageData = Buffer.alloc(width * height * bytesPerPixel);

  const pad = Math.round(width * 0.0625);
  const radius = Math.round(width * 0.21875);
  drawRoundedRect(
    imageData,
    width,
    height,
    { x: pad, y: pad, w: width - pad * 2, h: height - pad * 2 },
    radius,
    [249, 115, 22, 255],
  );

  const wStroke = Math.max(3, width * 0.11);
  drawStroke(
    imageData,
    width,
    height,
    [
      [width * 0.25, height * 0.27],
      [width * 0.34, height * 0.72],
      [width * 0.5, height * 0.39],
      [width * 0.66, height * 0.72],
      [width * 0.75, height * 0.27],
    ],
    wStroke,
    [255, 255, 255, 255],
  );

  // Prepare scanlines with filter byte (0 = none)
  const scanlines = [];
  for (let y = 0; y < height; y++) {
    const scanline = Buffer.alloc(1 + width * bytesPerPixel);
    scanline[0] = 0; // filter type
    imageData.copy(scanline, 1, y * width * bytesPerPixel, (y + 1) * width * bytesPerPixel);
    scanlines.push(scanline);
  }

  const raw_data = Buffer.concat(scanlines);
  const idat_data = deflateSync(raw_data);

  // Create PNG chunks
  const ihdr_chunk = createChunk('IHDR', ihdr);
  const idat_chunk = createChunk('IDAT', idat_data);
  const iend_chunk = createChunk('IEND', Buffer.alloc(0));

  return Buffer.concat([
    PNG_SIGNATURE,
    ihdr_chunk,
    idat_chunk,
    iend_chunk
  ]);
}

function generateIcons() {
  console.log('Generating icons...');

  sizes.forEach(({ size, name }) => {
    const buffer = createMinimalPNG(size, size);
    const filePath = join(iconsDir, name);
    writeFileSync(filePath, buffer);
    console.log(`  ✓ Generated ${name}`);
  });

  console.log('\n✅ PNG icons generated successfully!');
  console.log('Note: Run "node generate-ico.js" to create the Windows .ico file.');
}

generateIcons();
