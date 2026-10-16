#!/usr/bin/env node
//
// QEMU screendump helpers.
//
// A screendump is a binary PPM (P6) file. `parsePpm` turns it into
// something a test can query pixel by pixel, and `encodePng` writes a
// real PNG so a human can open the frame in an image viewer.
//
// Usage:
//
//     node tests/screendump.mjs <input.ppm> <output.png>
//

import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

// ============================================================
// PPM
// ============================================================

export function parsePpm(file) {
    const bytes = fs.readFileSync(file);

    let offset = 0;

    const nextToken = () => {
        while (offset < bytes.length) {
            const byte = bytes[offset];

            if (byte === 0x23) {
                while (offset < bytes.length && bytes[offset] !== 0x0a) {
                    offset += 1;
                }
            } else if (byte === 0x20 || byte === 0x09 || byte === 0x0a || byte === 0x0d) {
                offset += 1;
            } else {
                break;
            }
        }

        const start = offset;

        while (offset < bytes.length && ![0x20, 0x09, 0x0a, 0x0d].includes(bytes[offset])) {
            offset += 1;
        }

        return bytes.toString('ascii', start, offset);
    };

    const magic = nextToken();

    if (magic !== 'P6') {
        throw new Error(`unsupported screendump format: ${magic}`);
    }

    const width = Number(nextToken());
    const height = Number(nextToken());
    const maxValue = Number(nextToken());

    if (maxValue !== 255) {
        throw new Error(`unsupported PPM max value: ${maxValue}`);
    }

    // A single whitespace byte separates the header from the pixels.
    offset += 1;

    const pixels = bytes.subarray(offset, offset + width * height * 3);

    if (pixels.length < width * height * 3) {
        throw new Error('truncated screendump');
    }

    return {
        width,
        height,

        pixel(x, y) {
            const index = (y * width + x) * 3;

            return [pixels[index], pixels[index + 1], pixels[index + 2]];
        },
    };
}

// ============================================================
// PNG
// ============================================================

function crc32(buffer) {
    let crc = 0xffffffff;

    for (const byte of buffer) {
        crc ^= byte;

        for (let bit = 0; bit < 8; bit += 1) {
            crc = crc & 1 ? (crc >>> 1) ^ 0xedb88320 : crc >>> 1;
        }
    }

    return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
    const length = Buffer.alloc(4);

    length.writeUInt32BE(data.length);

    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);

    const checksum = Buffer.alloc(4);

    checksum.writeUInt32BE(crc32(body));

    return Buffer.concat([length, body, checksum]);
}

export function writePng(file, image) {
    const { width, height } = image;

    // One filter byte (0 = none) per row, then RGB triples.
    const raw = Buffer.alloc(height * (width * 3 + 1));

    for (let y = 0; y < height; y += 1) {
        const rowStart = y * (width * 3 + 1);

        raw[rowStart] = 0;

        for (let x = 0; x < width; x += 1) {
            const [red, green, blue] = image.pixel(x, y);

            const target = rowStart + 1 + x * 3;

            raw[target] = red;
            raw[target + 1] = green;
            raw[target + 2] = blue;
        }
    }

    const header = Buffer.alloc(13);

    header.writeUInt32BE(width, 0);
    header.writeUInt32BE(height, 4);
    header[8] = 8; // bit depth
    header[9] = 2; // truecolor
    header[10] = 0; // deflate
    header[11] = 0; // adaptive filtering
    header[12] = 0; // no interlace

    const png = Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk('IHDR', header),
        chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
        chunk('IEND', Buffer.alloc(0)),
    ]);

    fs.mkdirSync(path.dirname(path.resolve(file)), { recursive: true });
    fs.writeFileSync(file, png);

    return png.length;
}

// ============================================================
// CLI
// ============================================================

const invokedDirectly =
    process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename);

if (invokedDirectly) {
    const [input, output] = process.argv.slice(2);

    if (!input || !output) {
        console.error('usage: node tests/screendump.mjs <input.ppm> <output.png>');
        process.exit(2);
    }

    const image = parsePpm(input);
    const size = writePng(output, image);

    console.log(`wrote ${output} (${image.width}x${image.height}, ${size} bytes)`);
}
