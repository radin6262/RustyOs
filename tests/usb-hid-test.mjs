// ============================================================
// RustyOS non-boot HID test
// ============================================================
//
// Attaches a QEMU USB tablet, which has no HID boot interface. The driver
// must parse its report descriptor, classify it as a pointing device and
// scale its absolute coordinates onto the framebuffer.
//
// Usage:
//
//     node tests/usb-hid-test.mjs
//

import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';
import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';

import { parsePpm } from './screendump.mjs';

const REPO = path.resolve(import.meta.dirname, '..');

const QEMU = 'C:\\Program Files\\qemu\\qemu-system-x86_64.exe';
const BIOS = 'C:\\Program Files\\qemu\\OVMF.fd';

const OUT_DIR = path.join(REPO, 'target', 'usb-hid-test');
const SERIAL_LOG = path.join(OUT_DIR, 'serial.log');
const SCREENSHOT = path.join(OUT_DIR, 'screen.ppm');
const IDLE_SCREENSHOT = path.join(OUT_DIR, 'screen-idle.ppm');

const QMP_PORT = 45462;

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

function serialText() {
    try {
        return fs.readFileSync(SERIAL_LOG, 'utf8');
    } catch {
        return '';
    }
}

async function waitForSerial(pattern, timeoutMs, label) {
    const deadline = Date.now() + timeoutMs;

    while (Date.now() < deadline) {
        if (pattern.test(serialText())) {
            return true;
        }

        await sleep(200);
    }

    throw new Error(`timed out waiting for ${label ?? pattern}`);
}

function countBright(image, rect) {
    let count = 0;

    for (let y = rect.y; y < rect.y + rect.height; y += 1) {
        for (let x = rect.x; x < rect.x + rect.width; x += 1) {
            const [red, green, blue] = image.pixel(x, y);

            if (red > 200 && green > 200 && blue > 200) {
                count += 1;
            }
        }
    }

    return count;
}

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
        });
    }

    static async connect(port) {
        const deadline = Date.now() + 30_000;

        for (;;) {
            try {
                const socket = await new Promise((resolve, reject) => {
                    const client = net.createConnection({ port, host: '127.0.0.1' }, () =>
                        resolve(client),
                    );

                    client.on('error', reject);
                });

                const qmp = new Qmp(socket);

                await new Promise((resolve) => socket.once('data', resolve));

                return qmp;
            } catch {
                if (Date.now() >= deadline) {
                    throw new Error('could not connect to QMP');
                }

                await sleep(250);
            }
        }
    }
}

async function main() {
    const image = findImage();

    fs.rmSync(OUT_DIR, { recursive: true, force: true });
    fs.mkdirSync(OUT_DIR, { recursive: true });
    fs.writeFileSync(SERIAL_LOG, '');

    console.log('RustyOS non-boot HID test');
    console.log(`  image:  ${image}`);
    console.log(`  serial: ${SERIAL_LOG}`);
    console.log('');

    const qemu = spawn(
        QEMU,
        [
            '-machine', 'q35',
            '-m', '512M',
            '-bios', BIOS,
            '-device', 'qemu-xhci,id=xhci',
            '-device', 'usb-tablet,bus=xhci.0',
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
        await waitForSerial(/USB: HID mouse .* mode=report/, 120_000, 'report descriptor');

        const log = serialText();

        check(
            'non-boot HID interface uses the report descriptor',
            /USB: HID mouse .* mode=report/.test(log),
            'no boot protocol requested',
        );

        qmp = await Qmp.connect(QMP_PORT);

        await qmp.command('qmp_capabilities');

        // Move to 61% of the tablet's logical range in both axes. On a
        // 1280x800 screen the compositor cursor should land around
        // (781, 488), inside the empty terminal area.
        await qmp.command('input-send-event', {
            events: [
                { type: 'abs', data: { axis: 'x', value: 20000 } },
                { type: 'abs', data: { axis: 'y', value: 20000 } },
            ],
        });

        await waitForSerial(/USB: mouse report received bytes=0x0*6/, 10_000, 'tablet report');

        check(
            'tablet report is decoded',
            /USB: mouse report received bytes=0x0*6/.test(serialText()),
            '6-byte absolute report',
        );

        await sleep(500);

        await qmp.command('screendump', { filename: SCREENSHOT });

        await sleep(500);

        const image = parsePpm(SCREENSHOT);

        const expected = {
            x: Math.round((20000 / 32767) * image.width),
            y: Math.round((20000 / 32767) * image.height),
            width: 28,
            height: 28,
        };

        expected.x -= 14;
        expected.y -= 11;

        const center = {
            x: image.width / 2 - 20,
            y: image.height / 2 - 20,
            width: 40,
            height: 40,
        };

        const cursorPixels = countBright(image, expected);
        const centerPixels = countBright(image, center);

        check(
            'absolute coordinates move the cursor to the scaled position',
            cursorPixels > 10 && centerPixels === 0,
            `cursor box ${cursorPixels} bright pixels, centre box ${centerPixels}`,
        );

        // ----------------------------------------------------
        // The idle desktop must keep polling USB after the last
        // application quits.
        //
        // The xHCI driver runs in polled mode (no interrupter), so a
        // sleeping CPU would never see another USB report. Closing
        // Terminal must leave a desktop whose pointer still moves.
        // ----------------------------------------------------

        const sendAbsolute = async (x, y) => {
            await qmp.command('input-send-event', {
                events: [
                    {
                        type: 'abs',
                        data: {
                            axis: 'x',
                            value: Math.round((x / image.width) * 32767),
                        },
                    },
                    {
                        type: 'abs',
                        data: {
                            axis: 'y',
                            value: Math.round((y / image.height) * 32767),
                        },
                    },
                ],
            });
        };

        const sendButton = async (down) => {
            await qmp.command('input-send-event', {
                events: [{ type: 'btn', data: { down, button: 'left' } }],
            });
        };

        // Terminal's red traffic light: (290 + 18, 175 - 30 + 15).
        await sendAbsolute(308, 160);
        await sleep(400);
        await sendButton(true);
        await sleep(80);
        await sendButton(false);

        await waitForSerial(/DESKTOP: idle/, 30_000, 'idle desktop');

        const idleX = 300;
        const idleY = 650;

        await sendAbsolute(idleX, idleY);
        await sleep(1500);

        await qmp.command('screendump', { filename: IDLE_SCREENSHOT });

        const idleImage = parsePpm(IDLE_SCREENSHOT);

        const idleCursor = { x: idleX - 4, y: idleY - 4, width: 30, height: 30 };
        const oldCursor = { x: 308 - 4, y: 160 - 4, width: 30, height: 30 };

        const movedPixels = countBright(idleImage, idleCursor);
        const stalePixels = countBright(idleImage, oldCursor);

        check(
            'USB pointer still moves on the idle desktop after the app quits',
            movedPixels > 10 && stalePixels === 0,
            `${movedPixels} bright pixels at the new position, ${stalePixels} at the old one`,
        );
    } catch (error) {
        check('test completed without a harness error', false, error.message);
    } finally {
        if (qemuOutput.trim().length > 0) {
            fs.writeFileSync(path.join(OUT_DIR, 'qemu.log'), qemuOutput);
        }

        try {
            qmp?.socket.destroy();
        } catch {
            // ignore
        }

        qemu.kill();

        await sleep(500);
    }

    const failed = results.filter((result) => !result.ok);

    console.log('');
    console.log(`${results.length - failed.length}/${results.length} checks passed`);

    if (failed.length > 0) {
        process.exitCode = 1;
    }
}

await main();
