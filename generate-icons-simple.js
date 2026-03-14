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

  // Create image data - simple gradient
  const bytesPerPixel = 4;
  const imageData = Buffer.alloc(width * height * bytesPerPixel);

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const idx = (y * width + x) * 4;

      // Create a simple gradient from blue to purple
      const ratio = (x + y) / (width + height);
      imageData[idx] = Math.floor(102 + ratio * 50);     // R
      imageData[idx + 1] = Math.floor(126 + ratio * 50); // G
      imageData[idx + 2] = Math.floor(234 - ratio * 100); // B
      imageData[idx + 3] = 255; // A
    }
  }

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

console.log('Generating icons...');

sizes.forEach(({ size, name }) => {
  const buffer = createMinimalPNG(size, size);
  const filePath = join(iconsDir, name);
  writeFileSync(filePath, buffer);
  console.log(`  ✓ Generated ${name}`);
});

console.log('\n✅ Icons generated successfully!');
