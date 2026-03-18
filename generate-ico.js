import { writeFileSync, readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const iconsDir = join(__dirname, 'src-tauri', 'icons');

// Use the existing 256x256 PNG as the ICO payload.
const pngPath = join(iconsDir, '128x128@2x.png');
const pngData = readFileSync(pngPath);

const ICO_HEADER_SIZE = 6;
const ICO_DIR_ENTRY_SIZE = 16;

const icoHeader = Buffer.alloc(ICO_HEADER_SIZE);
icoHeader.writeUInt16LE(0, 0); // reserved
icoHeader.writeUInt16LE(1, 2); // type: icon
icoHeader.writeUInt16LE(1, 4); // image count

const dirEntry = Buffer.alloc(ICO_DIR_ENTRY_SIZE);
dirEntry.writeUInt8(0, 0); // width: 0 => 256
dirEntry.writeUInt8(0, 1); // height: 0 => 256
dirEntry.writeUInt8(0, 2); // palette colors
dirEntry.writeUInt8(0, 3); // reserved
dirEntry.writeUInt16LE(1, 4); // color planes
dirEntry.writeUInt16LE(32, 6); // bits per pixel
dirEntry.writeUInt32LE(pngData.length, 8); // image bytes
dirEntry.writeUInt32LE(ICO_HEADER_SIZE + ICO_DIR_ENTRY_SIZE, 12); // image offset

const icoFile = Buffer.concat([icoHeader, dirEntry, pngData]);
const icoPath = join(iconsDir, 'icon.ico');
writeFileSync(icoPath, icoFile);

console.log('Generated icon.ico');
