import { writeFileSync, readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const iconsDir = join(__dirname, 'src-tauri', 'icons');

// Read the 256x256 PNG file
const pngPath = join(iconsDir, '128x128@2x.png');
const pngData = readFileSync(pngPath);

// Remove PNG signature (first 8 bytes)
const pngBody = pngData.slice(8);

// Create ICO header
const ICO_HEADER_SIZE = 6;
const ICO_DIR_ENTRY_SIZE = 16;

// ICO header
const icoHeader = Buffer.alloc(ICO_HEADER_SIZE);
icoHeader.writeUInt16LE(0, 0);      // Reserved (must be 0)
icoHeader.writeUInt16LE(1, 2);      // Image type (1 = ICO)
icoHeader.writeUInt16LE(1, 4);      // Number of images (1)

// Directory entry
const dirEntry = Buffer.alloc(ICO_DIR_ENTRY_SIZE);
dirEntry.writeUInt8(0, 0);          // Width (0 = 256)
dirEntry.writeUInt8(0, 1);          // Height (0 = 256)
dirEntry.writeUInt8(0, 2);          // Color palette count (0 = no palette)
dirEntry.writeUInt8(0, 3);          // Reserved
dirEntry.writeUInt16LE(1, 4);       // Color planes (should be 1 or 0)
dirEntry.writeUInt16LE(32, 6);      // Bits per pixel (32 for RGBA)
dirEntry.writeUInt32LE(pngBody.length, 8);  // Size of image data
dirEntry.writeUInt32LE(ICO_HEADER_SIZE + ICO_DIR_ENTRY_SIZE, 12);  // Offset to image data

// Combine everything
const icoFile = Buffer.concat([icoHeader, dirEntry, pngBody]);

// Write ICO file
const icoPath = join(iconsDir, 'icon.ico');
writeFileSync(icoPath, icoFile);

console.log('✅ Generated icon.ico');
