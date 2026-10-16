#!/usr/bin/env node
//
// RustyOS desktop test.
//
// Boots the freshly built UEFI image headlessly in QEMU and checks the
// desktop end to end:
//
//   1. the kernel reports a page-table verified shared mapping for the
//      window the application created
//   2. the application never calls the legacy per-rectangle syscalls,
//      yet its window content is on screen
//   3. the macOS style chrome is composited: menu bar, window title bar
//      with traffic lights, rounded window corners and a dock pinned to
//      the right edge
//   4. typing a key reaches the application and is drawn through the
//      same shared buffer
//   5. moving the pointer over the dock shows that item's hover label
//
// All screen checks use QEMU screendumps, so they verify what the user
// actually sees rather than only that a syscall returned.
//
// Usage:
//
//     node tests/shm-test.mjs [--keep] [--timeout=SECONDS]
//
// Build order matters: the kernel embeds the userspace ELF, so run
// `apps.ps1` before `cargo build`.
//

import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';
import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';

import { parsePpm, writePng } from './screendump.mjs';

const REPO = path.resolve(import.meta.dirname, '..');

const QEMU = 'C:\\Program Files\\qemu\\qemu-system-x86_64.exe';
const BIOS = 'C:\\Program Files\\qemu\\OVMF.fd';

const OUT_DIR = path.join(REPO, 'target', 'shm-test');
const SERIAL_LOG = path.join(OUT_DIR, 'serial.log');

const QMP_PORT = 45454;

const KEEP = process.argv.includes('--keep');

const timeoutArgument = process.argv.find((value) => value.startsWith('--timeout='));

const BOOT_TIMEOUT_MS = timeoutArgument
    ? Number(timeoutArgument.split('=')[1]) * 1000
    : 90_000;

// The unoptimized kernel rasterizes its font in software and builds the
// wallpaper pixel by pixel, so the first frame can take a while.
const FRAME_TIMEOUT_MS = 60_000;

// ============================================================
// Terminal window geometry
// ============================================================
//
// user/terminal/src/main.rs creates its window at:
//
//     position = (290, 175)
//     size     = (700, 450)
//
// with a 30 pixel title bar drawn above it by the compositor's chrome.

const WINDOW_X = 290;
const WINDOW_Y = 175;
const WINDOW_WIDTH = 700;
const WINDOW_HEIGHT = 450;

const TITLE_BAR_HEIGHT = 30;

const COLOR_BLACK = [0, 0, 0];

// The kernel's font rasterizer antialiases glyphs, so terminal text is a
// range from dark gray to white rather than pure white. Anything
// noticeably brighter than the black window background counts as text.
const TEXT_BRIGHTNESS = 150;

// macOS traffic light colors.
const TRAFFIC_LIGHTS = [
    [0xff, 0x5f, 0x57],
    [0xfe, 0xbc, 0x2e],
    [0x28, 0xc8, 0x40],
];

// Dock icon accent colors.
const DOCK_ACCENTS = [
    [0x0e, 0xa5, 0xe9],
    [0xec, 0x48, 0x99],
    [0xe5, 0xe7, 0xeb],
];

// Where the first typed character is drawn by the terminal:
//
//     local x = CHAR_WIDTH + PROMPT_GAP + TEXT_X = 32
//     local y = TEXT_Y + 3 * LINE_HEIGHT + PROMPT_GAP_Y = 76
//
// The ">" prompt occupies local x 20..29, so this box covers the typed
// character only.
const TYPED_CHARACTER_BOX = {
    x: WINDOW_X + 31,
    y: WINDOW_Y + 75,
    width: 22,
    height: 20,
};

const PROMPT_LINE = {
    x: WINDOW_X + 3,
    y: WINDOW_Y + 60,
    width: WINDOW_WIDTH - 6,
    height: 40,
};

const results = [];

function check(name, ok, detail) {
    results.push({ name, ok, detail });

    console.log(`  [${ok ? 'PASS' : 'FAIL'}] ${name}${detail ? ` -- ${detail}` : ''}`);
}

function findImage() {
    const buildDir = path.join(REPO, 'target', 'debug', 'build');

    if (!fs.existsSync(buildDir)) {
        throw new Error(`no build directory at ${buildDir}; run cargo build first`);
    }

    const candidates = [];

    for (const entry of fs.readdirSync(buildDir)) {
        const image = path.join(buildDir, entry, 'out', 'rusty-uefi.img');

        if (fs.existsSync(image)) {
            candidates.push(image);
        }
    }

    if (candidates.length === 0) {
        throw new Error('no rusty-uefi.img found; run cargo build first');
    }

    candidates.sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);

    return candidates[0];
}

// ============================================================
// QMP client
// ============================================================

class Qmp {
    constructor(socket) {
        this.socket = socket;
        this.buffer = '';
        this.pending = new Map();
        this.nextId = 1;

        socket.on('data', (chunk) => this.onData(chunk));
    }

    onData(chunk) {
        this.buffer += chunk.toString('utf8');

        let newline;

        while ((newline = this.buffer.indexOf('\n')) >= 0) {
            const line = this.buffer.slice(0, newline).trim();

            this.buffer = this.buffer.slice(newline + 1);

            if (line.length === 0) {
                continue;
            }

            let message;

            try {
                message = JSON.parse(line);
            } catch {
                continue;
            }

            if (message.id && this.pending.has(message.id)) {
                const { resolve, reject } = this.pending.get(message.id);

                this.pending.delete(message.id);

                if (message.error) {
                    reject(new Error(JSON.stringify(message.error)));
                } else {
                    resolve(message.return);
                }
            }
        }
    }

    command(execute, args) {
        const id = this.nextId++;

        const request = { execute, id };

        if (args) {
            request.arguments = args;
        }

        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });

            this.socket.write(`${JSON.stringify(request)}\n`);

            setTimeout(() => {
                if (this.pending.has(id)) {
                    this.pending.delete(id);

                    reject(new Error(`QMP ${execute} timed out`));
                }
            }, 20_000);
        });
    }

    static async connect(port) {
        const deadline = Date.now() + 30_000;

        let socket;

        for (;;) {
            try {
                socket = await new Promise((resolve, reject) => {
                    const candidate = net.connect({ host: '127.0.0.1', port }, () =>
                        resolve(candidate),
                    );

                    candidate.once('error', reject);
                });

                break;
            } catch (error) {
                if (Date.now() > deadline) {
                    throw new Error(`could not open QMP socket: ${error.message}`);
                }

                await sleep(150);
            }
        }

        // The greeting arrives unprompted.
        await sleep(200);

        const qmp = new Qmp(socket);

        await qmp.command('qmp_capabilities');

        return qmp;
    }

    close() {
        this.socket.destroy();
    }
}

// ============================================================
// Serial log
// ============================================================

function readSerial() {
    if (!fs.existsSync(SERIAL_LOG)) {
        return '';
    }

    return fs.readFileSync(SERIAL_LOG, 'utf8');
}

async function waitForSerial(marker, timeoutMs) {
    const deadline = Date.now() + timeoutMs;

    for (;;) {
        const log = readSerial();

        if (log.includes(marker)) {
            return log;
        }

        if (Date.now() > deadline) {
            return null;
        }

        await sleep(100);
    }
}

/// The kernel reports its chrome geometry once at startup:
///
///     WM: chrome ready menu-bar=0x1A dock=0x50x0x1EA at (0x49E,0x9B)
///
/// `write_usize` prints hexadecimal, so every number is base 16.
function parseChromeGeometry(log) {
    const match = (log ?? '').match(
        /WM: chrome ready menu-bar=(0x[0-9a-f]+) dock=(0x[0-9a-f]+)x(0x[0-9a-f]+) at \((0x[0-9a-f]+),(0x[0-9a-f]+)\)/i,
    );

    if (!match) {
        return null;
    }

    return {
        menuBarHeight: Number(match[1]),
        dock: {
            width: Number(match[2]),
            height: Number(match[3]),
            x: Number(match[4]),
            y: Number(match[5]),
        },
    };
}

// ============================================================
// Screendumps
// ============================================================

async function capture(qmp, name) {
    const file = path.join(OUT_DIR, name);

    await qmp.command('screendump', { filename: file });

    return parsePpm(file);
}

/// Capture frames until `predicate` holds.
///
/// Returns the matching frame, or the last captured frame with
/// `matched: false` on timeout: the caller reports that as a failed check
/// instead of aborting the run.
async function pollFrame(qmp, name, predicate, timeoutMs) {
    const deadline = Date.now() + timeoutMs;

    let image = await capture(qmp, name);

    while (!predicate(image)) {
        if (Date.now() > deadline) {
            return { image, matched: false };
        }

        await sleep(400);

        image = await capture(qmp, name);
    }

    return { image, matched: true };
}

// ============================================================
// Pixel helpers
// ============================================================

function sameColor(a, b) {
    return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

function closeTo(pixel, color, tolerance) {
    return (
        Math.abs(pixel[0] - color[0]) <= tolerance &&
        Math.abs(pixel[1] - color[1]) <= tolerance &&
        Math.abs(pixel[2] - color[2]) <= tolerance
    );
}

function brightness(pixel) {
    return (pixel[0] + pixel[1] + pixel[2]) / 3;
}

function isText(pixel) {
    return brightness(pixel) * 3 >= TEXT_BRIGHTNESS;
}

const WINDOW_INSIDE = {
    x: WINDOW_X + 3,
    y: WINDOW_Y + 3,
    width: WINDOW_WIDTH - 6,
    height: WINDOW_HEIGHT - 6,
};

function countPixels(image, rect, predicate) {
    let count = 0;

    const x0 = Math.max(0, rect.x);
    const y0 = Math.max(0, rect.y);

    const x1 = Math.min(image.width, rect.x + rect.width);
    const y1 = Math.min(image.height, rect.y + rect.height);

    for (let y = y0; y < y1; y += 1) {
        for (let x = x0; x < x1; x += 1) {
            if (predicate(image.pixel(x, y), x, y)) {
                count += 1;
            }
        }
    }

    return count;
}

function countChanged(before, after, rect) {
    let count = 0;

    for (let y = rect.y; y < rect.y + rect.height; y += 1) {
        for (let x = rect.x; x < rect.x + rect.width; x += 1) {
            if (!sameColor(before.pixel(x, y), after.pixel(x, y))) {
                count += 1;
            }
        }
    }

    return count;
}

// ============================================================
// Main
// ============================================================

async function main() {
    const image = findImage();

    fs.rmSync(OUT_DIR, { recursive: true, force: true });
    fs.mkdirSync(OUT_DIR, { recursive: true });
    fs.writeFileSync(SERIAL_LOG, '');

    console.log('RustyOS desktop test');
    console.log(`  image:  ${image}`);
    console.log(`  serial: ${SERIAL_LOG}`);
    console.log('');

    const qemu = spawn(
        QEMU,
        [
            '-machine', 'q35',
            '-m', '512M',
            '-bios', BIOS,
            '-drive', `format=raw,file=${image}`,
            '-display', 'none',
            '-serial', `file:${SERIAL_LOG}`,
            '-qmp', `tcp:127.0.0.1:${QMP_PORT},server,nowait`,
            '-no-reboot',
        ],
        { stdio: ['ignore', 'pipe', 'pipe'] },
    );

    let qemuOutput = '';

    qemu.stdout.on('data', (chunk) => {
        qemuOutput += chunk.toString('utf8');
    });

    qemu.stderr.on('data', (chunk) => {
        qemuOutput += chunk.toString('utf8');
    });

    let qmp = null;

    try {
        // ----------------------------------------------------
        // Boot
        // ----------------------------------------------------

        const bootLog = await waitForSerial('ELF LAUNCH: entering ring3', BOOT_TIMEOUT_MS);

        check(
            'kernel reaches the userspace terminal',
            bootLog !== null,
            bootLog === null ? 'serial log has no ring 3 entry' : 'ELF launched into Ring 3',
        );

        if (bootLog === null) {
            throw new Error('boot did not reach userspace');
        }

        // ----------------------------------------------------
        // Shared buffer mapping
        // ----------------------------------------------------

        const log = await waitForSerial('SHARED MEM: window 0x', 30_000);

        check(
            'kernel maps a shared buffer for the window',
            log !== null,
            log === null ? 'no mapping reported' : 'window mapping reported',
        );

        const mappingLine = log?.match(
            /SHARED MEM: window 0x[0-9a-f]+ VA=0x[0-9a-f]+ PA=0x[0-9a-f]+ bytes=0x[0-9a-f]+ BACKED/i,
        )?.[0];

        check(
            'shared mapping verified by a page-table walk',
            Boolean(mappingLine),
            mappingLine ?? 'mapping was not verified',
        );

        const mappingFailures = (log ?? '')
            .split('\n')
            .filter(
                (line) =>
                    line.includes('SHARED MEM') &&
                    (line.includes('FAILED') || line.includes('failed')),
            );

        check(
            'kernel reports no shared mapping failure',
            mappingFailures.length === 0,
            mappingFailures.length === 0 ? 'clean' : mappingFailures.join(' | '),
        );

        // ----------------------------------------------------
        // Chrome geometry
        // ----------------------------------------------------

        const chrome = parseChromeGeometry(log);

        check(
            'compositor reports its chrome geometry',
            chrome !== null && chrome.dock.width > 0,
            chrome === null
                ? 'no chrome geometry in the serial log'
                : `menu bar ${chrome.menuBarHeight}px, dock ${chrome.dock.width}x${chrome.dock.height} at (${chrome.dock.x},${chrome.dock.y})`,
        );

        // ----------------------------------------------------
        // First frame
        // ----------------------------------------------------

        qmp = await Qmp.connect(QMP_PORT);

        const windowDrawn = (frame) =>
            sameColor(frame.pixel(WINDOW_X + 10, WINDOW_Y + 10), COLOR_BLACK) &&
            sameColor(frame.pixel(WINDOW_X + 100, WINDOW_Y + 40), COLOR_BLACK);

        const firstFrame = await pollFrame(qmp, 'before.ppm', windowDrawn, FRAME_TIMEOUT_MS);

        const before = firstFrame.image;

        check(
            'compositor presents the application window',
            firstFrame.matched,
            firstFrame.matched
                ? `window visible on ${before.width}x${before.height} screen`
                : 'no frame with the window appeared',
        );

        const screenFits =
            before.width >= WINDOW_X + WINDOW_WIDTH &&
            before.height >= WINDOW_Y + WINDOW_HEIGHT &&
            chrome !== null;

        check(
            'screendump covers the desktop chrome',
            screenFits,
            `${before.width}x${before.height}`,
        );

        if (!screenFits) {
            return;
        }

        writePng(path.join(OUT_DIR, 'screen.png'), before);

        // ----------------------------------------------------
        // The application must not draw through the legacy
        // per-rectangle syscalls any more.
        // ----------------------------------------------------

        const legacyFill = readSerial().includes('SYS_FILL_RECT syscall used by userspace');
        const legacyDrawRect = readSerial().includes('SYS_DRAW_RECT syscall used by userspace');

        check(
            'application draws rectangles without SYS_FILL_RECT/SYS_DRAW_RECT',
            !legacyFill && !legacyDrawRect,
            legacyFill || legacyDrawRect
                ? 'legacy rectangle syscall was used'
                : 'no legacy rectangle syscall in the serial log',
        );

        // ----------------------------------------------------
        // The screen must show what the application wrote into
        // the shared buffer.
        // ----------------------------------------------------

        const cursorX = Math.floor(before.width / 2);
        const cursorY = Math.floor(before.height / 2);

        const onCursor = (x, y) =>
            x >= cursorX - 2 && x <= cursorX + 16 && y >= cursorY - 2 && y <= cursorY + 20;

        const insidePixels = countPixels(before, WINDOW_INSIDE, () => true);

        const notBackground = countPixels(
            before,
            WINDOW_INSIDE,
            (pixel, x, y) => !onCursor(x, y) && !sameColor(pixel, COLOR_BLACK),
        );

        check(
            'window background reached the screen from the shared buffer',
            notBackground / insidePixels < 0.05,
            `${notBackground} of ${insidePixels} pixels are not the terminal background`,
        );

        const textPixels = countPixels(
            before,
            WINDOW_INSIDE,
            (pixel, x, y) => !onCursor(x, y) && isText(pixel),
        );

        check(
            'terminal text is present inside the window',
            textPixels > 0,
            `${textPixels} text pixels`,
        );

        // ----------------------------------------------------
        // macOS style chrome
        // ----------------------------------------------------

        const menuBar = {
            x: 0,
            y: 0,
            width: before.width,
            height: chrome.menuBarHeight,
        };

        const menuBarDark = countPixels(
            before,
            menuBar,
            (pixel) => brightness(pixel) < 110,
        );

        const menuBarPixels = countPixels(before, menuBar, () => true);

        check(
            'translucent menu bar spans the top edge',
            menuBarDark / menuBarPixels > 0.95,
            `${menuBarDark} of ${menuBarPixels} menu bar pixels are dark`,
        );

        const logoPixels = countPixels(
            before,
            { x: 8, y: 0, width: 34, height: chrome.menuBarHeight },
            (pixel) => brightness(pixel) > 170,
        );

        check(
            'menu bar logo is drawn',
            logoPixels > 40,
            `${logoPixels} logo pixels`,
        );

        const statusPixels = countPixels(
            before,
            {
                x: before.width - 80,
                y: 0,
                width: 70,
                height: chrome.menuBarHeight,
            },
            (pixel) => brightness(pixel) > 140,
        );

        check(
            'menu bar status glyphs are drawn',
            statusPixels > 40,
            `${statusPixels} status pixels`,
        );

        const titleBar = {
            x: WINDOW_X + 100,
            y: WINDOW_Y - TITLE_BAR_HEIGHT + 3,
            width: WINDOW_WIDTH - 200,
            height: TITLE_BAR_HEIGHT - 6,
        };

        check(
            'window title bar sits above the content',
            sameColor(before.pixel(WINDOW_X + 100, WINDOW_Y - 10), COLOR_BLACK) === false &&
                brightness(before.pixel(WINDOW_X + 100, WINDOW_Y - 10)) < 90,
            `title bar rgb(${before.pixel(WINDOW_X + 100, WINDOW_Y - 10).join(',')})`,
        );

        const trafficLightArea = {
            x: WINDOW_X,
            y: WINDOW_Y - TITLE_BAR_HEIGHT,
            width: 90,
            height: TITLE_BAR_HEIGHT,
        };

        const lightCounts = TRAFFIC_LIGHTS.map((color) =>
            countPixels(before, trafficLightArea, (pixel) => closeTo(pixel, color, 10)),
        );

        check(
            'macOS traffic lights are drawn in the title bar',
            lightCounts.every((count) => count >= 20),
            `close/minimize/zoom pixels: ${lightCounts.join('/')}`,
        );

        // Rounded frame: the very corner of the window frame is not
        // content, while a few pixels inside it still is.
        const cornerPixel = before.pixel(WINDOW_X + 1, WINDOW_Y + WINDOW_HEIGHT - 2);

        const cornerInside = before.pixel(WINDOW_X + 8, WINDOW_Y + WINDOW_HEIGHT - 8);

        check(
            'window corners are rounded',
            !sameColor(cornerPixel, COLOR_BLACK) && sameColor(cornerInside, COLOR_BLACK),
            `corner rgb(${cornerPixel.join(',')}), inside rgb(${cornerInside.join(',')})`,
        );

        // ----------------------------------------------------
        // Dock
        // ----------------------------------------------------

        const dock = chrome.dock;

        check(
            'dock is pinned to the right edge',
            dock.x > before.width - 200 &&
                dock.x + dock.width <= before.width &&
                dock.y > chrome.menuBarHeight &&
                dock.y + dock.height <= before.height,
            `dock ${dock.width}x${dock.height} at (${dock.x},${dock.y}) on a ${before.width}px wide screen`,
        );

        const dockPanel = {
            x: dock.x,
            y: dock.y,
            width: dock.width,
            height: dock.height,
        };

        const dockPixels = countPixels(before, dockPanel, () => true);

        const dockDark = countPixels(before, dockPanel, (pixel) => brightness(pixel) < 90);

        check(
            'translucent dock panel is drawn',
            dockDark / dockPixels > 0.5,
            `${dockDark} of ${dockPixels} dock pixels are dark`,
        );

        const accentCounts = DOCK_ACCENTS.map((accent) =>
            countPixels(before, dockPanel, (pixel) => closeTo(pixel, accent, 45)),
        );

        check(
            'dock icons are drawn with their accent colors',
            accentCounts.filter((count) => count >= 100).length >= 2,
            `accent pixels (blue/pink/gray): ${accentCounts.join('/')}`,
        );

        // ----------------------------------------------------
        // Typing must reach the application and be drawn
        // through the same shared buffer.
        // ----------------------------------------------------

        const emptyBox = countPixels(before, TYPED_CHARACTER_BOX, (pixel) => isText(pixel));

        check(
            'prompt line is empty before typing',
            emptyBox === 0,
            `${emptyBox} text pixels where the typed character goes`,
        );

        const sendKey = async (down) => {
            await qmp.command('input-send-event', {
                events: [
                    {
                        type: 'key',
                        data: {
                            down,
                            key: { type: 'qcode', data: 'a' },
                        },
                    },
                ],
            });
        };

        await sendKey(true);
        await sleep(60);
        await sendKey(false);

        const typedFrame = await pollFrame(
            qmp,
            'after.ppm',
            (frame) => countPixels(frame, TYPED_CHARACTER_BOX, (pixel) => isText(pixel)) > 0,
            20_000,
        );

        const typed = typedFrame.image;

        const typedPixels = countPixels(typed, TYPED_CHARACTER_BOX, (pixel) => isText(pixel));

        check(
            'typed key reaches the application and is drawn',
            typedPixels > 0,
            typedPixels > 0
                ? `${typedPixels} text pixels in the typed character box`
                : 'nothing appeared where the typed character goes',
        );

        const changed = countChanged(before, typed, PROMPT_LINE);

        check(
            'keystroke repaints the prompt line through the shared buffer',
            changed > 0,
            `${changed} pixels changed on the prompt line`,
        );

        // ----------------------------------------------------
        // Dock hover: move the pointer onto an icon, then force a
        // full composite with a keystroke and look for the label
        // beside the dock.
        // ----------------------------------------------------

        const hoverTargetY = dock.y + dock.height / 2;

        const pointerY = before.height / 2;

        // The dock is vertically centered, so only the horizontal
        // distance has to be travelled.
        let remaining = Math.round(dock.x + dock.width / 2 - before.width / 2);

        while (remaining !== 0) {
            const step = Math.max(-95, Math.min(95, remaining));

            await qmp.command('input-send-event', {
                events: [
                    {
                        type: 'rel',
                        data: { axis: 'x', value: step },
                    },
                ],
            });

            remaining -= step;

            await sleep(40);
        }

        check(
            'dock hover target is reachable from the screen center',
            Math.abs(hoverTargetY - pointerY) < dock.height / 2,
            `dock center y=${hoverTargetY}, pointer y=${pointerY}`,
        );

        // Beside the dock, but clear of the application window so the
        // terminal's own text cannot be mistaken for a hover label.
        const labelLeft = Math.max(
            dock.x - 220,
            WINDOW_X + WINDOW_WIDTH + 10,
        );

        const labelArea = {
            x: labelLeft,
            y: Math.round(hoverTargetY) - 20,
            width: Math.max(0, dock.x - 10 - labelLeft),
            height: 40,
        };

        // The label is a dark pill with white text drawn over the
        // wallpaper: look for darkness where the wallpaper was, plus
        // the text on top of it.
        const isDark = (pixel) => brightness(pixel) < 70;

        const isLabelText = (pixel) => brightness(pixel) > 200;

        const darkBefore = countPixels(typed, labelArea, (pixel) => isDark(pixel));

        await sendKey(true);
        await sleep(60);
        await sendKey(false);

        const hoverFrame = await pollFrame(
            qmp,
            'hover.ppm',
            (frame) => countPixels(frame, labelArea, (pixel) => isDark(pixel)) > darkBefore + 500,
            20_000,
        );

        const hovered = hoverFrame.image;

        const darkAfter = countPixels(hovered, labelArea, (pixel) => isDark(pixel));

        const labelText = countPixels(hovered, labelArea, (pixel) => isLabelText(pixel));

        check(
            'hovering a dock icon shows its label',
            darkAfter > darkBefore + 500 && labelText > 20,
            `${darkAfter} dark pill pixels (before: ${darkBefore}), ${labelText} label text pixels`,
        );

        writePng(path.join(OUT_DIR, 'screen-hover.png'), hovered);
    } finally {
        if (qmp) {
            qmp.close();
        }

        qemu.kill();

        await sleep(500);

        if (qemuOutput.trim().length > 0) {
            fs.writeFileSync(path.join(OUT_DIR, 'qemu.log'), qemuOutput);
        }
    }

    const failed = results.filter((result) => !result.ok);

    console.log('');
    console.log(`${results.length - failed.length}/${results.length} checks passed`);

    if (failed.length > 0) {
        console.log('');
        console.log('Failures:');

        for (const result of failed) {
            console.log(`  - ${result.name}: ${result.detail}`);
        }

        console.log('');
        console.log(`serial log: ${SERIAL_LOG}`);
        console.log(`screendumps: ${OUT_DIR}`);

        process.exitCode = 1;

        return;
    }

    if (!KEEP) {
        for (const frame of ['before.ppm', 'after.ppm', 'hover.ppm']) {
            fs.rmSync(path.join(OUT_DIR, frame), { force: true });
        }
    }

    console.log('');
    console.log('All desktop checks passed.');
    console.log(`serial log: ${SERIAL_LOG}`);
    console.log(`screenshots: ${path.join(OUT_DIR, 'screen.png')}, ${path.join(OUT_DIR, 'screen-hover.png')}`);
}

main().catch((error) => {
    console.error('');
    console.error(`desktop test failed: ${error.message}`);
    console.error('');

    process.exitCode = 1;
});
