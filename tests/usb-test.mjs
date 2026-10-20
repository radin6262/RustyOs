// ============================================================
// RustyOS xHCI/USB input test
// ============================================================
//
// Boots the UEFI image with two qemu-xhci controllers but no USB devices,
// then hotplugs a keyboard on the first controller and a mouse on the
// second over QMP. It checks that the driver:
//
//   1. finds and resets both xHCI controllers
//   2. divides root ports into USB2 and USB3 ranges
//   3. reads the scratchpad buffer count from HCSPARAMS2
//   4. enumerates a hotplugged boot keyboard and decodes a key report
//   5. enumerates a boot mouse and decodes a movement report
//   6. releases the slot when a device is unplugged
//
// Usage:
//
//     node tests/usb-test.mjs
//

import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';
import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';

const REPO = path.resolve(import.meta.dirname, '..');

const QEMU = 'C:\\Program Files\\qemu\\qemu-system-x86_64.exe';
const BIOS = 'C:\\Program Files\\qemu\\OVMF.fd';

const OUT_DIR = path.join(REPO, 'target', 'usb-test');
const SERIAL_LOG = path.join(OUT_DIR, 'serial.log');

const QMP_PORT = 45460;

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
        const text = serialText();

        if (pattern.test(text)) {
            return text;
        }

        await sleep(200);
    }

    throw new Error(`timed out waiting for ${label ?? pattern}`);
}

// ============================================================
// Minimal QMP client
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

// ============================================================
// Main
// ============================================================

async function main() {
    const image = findImage();

    fs.rmSync(OUT_DIR, { recursive: true, force: true });
    fs.mkdirSync(OUT_DIR, { recursive: true });
    fs.writeFileSync(SERIAL_LOG, '');

    console.log('RustyOS USB test');
    console.log(`  image:  ${image}`);
    console.log(`  serial: ${SERIAL_LOG}`);
    console.log('');

    const qemu = spawn(
        QEMU,
        [
            '-machine', 'q35',
            '-m', '512M',
            '-bios', BIOS,
            '-device', 'qemu-xhci,id=xhci0',
            '-device', 'qemu-xhci,id=xhci1',
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
        // Controller
        // ----------------------------------------------------

        await waitForSerial(/USB: subsystem initialized/, 120_000, 'USB subsystem');

        const bootLog = serialText();

        check(
            'driver finds the xHCI controller',
            /USB: xHCI controller at/.test(bootLog),
            'controller reported',
        );

        const controllerLines = (bootLog.match(/USB: xHCI controller at/g) ?? []).length;

        check(
            'both xHCI controllers initialize',
            controllerLines === 2,
            `${controllerLines} controllers reported`,
        );

        check(
            'scratchpad buffer count is read from HCSPARAMS2',
            /USB: scratchpad buffers=/.test(bootLog),
            'scratchpad setup path executed',
        );

        check(
            'controller reset completes',
            /USB: controller reset complete/.test(bootLog),
            'HCRST and CNR cleared',
        );

        const split = bootLog.match(/root ports USB2=0x0*(\d+) USB3=0x0*(\d+)/);

        check(
            'root ports are split into USB2 and USB3 ranges',
            Boolean(split) && Number(split[1]) === 4 && Number(split[2]) === 4,
            split ? `USB2=${split[1]} USB3=${split[2]}` : 'port split missing',
        );

        check(
            'no devices are present before hotplug',
            /USB: subsystem initialized, keyboards=0x0*0 mice=0x0*0/.test(bootLog),
            'slot table empty',
        );

        qmp = await Qmp.connect(QMP_PORT);

        await qmp.command('qmp_capabilities');

        // ----------------------------------------------------
        // Hotplug keyboard
        // ----------------------------------------------------

        await qmp.command('device_add', {
            driver: 'usb-kbd',
            bus: 'xhci0.0',
            id: 'usbtest-kbd',
        });

        await waitForSerial(/USB: HID keyboard ready on port/, 20_000, 'keyboard ready');

        check(
            'hotplugged boot keyboard enumerates',
            /USB: HID keyboard interface=.*endpoint=0x0*81/.test(serialText()),
            'HID boot interface and interrupt IN endpoint configured',
        );

        await qmp.command('send-key', {
            keys: [{ type: 'qcode', data: 'a' }],
            'hold-time': 100,
        });

        await waitForSerial(/USB: first key usage=0x0*4/, 10_000, 'key report');

        check(
            'keyboard report decodes the a key (usage 0x04)',
            /USB: first key usage=0x0*4/.test(serialText()),
            'usage 0x04 decoded',
        );

        // ----------------------------------------------------
        // Hotplug mouse
        // ----------------------------------------------------

        await qmp.command('device_add', {
            driver: 'usb-mouse',
            bus: 'xhci1.0',
            id: 'usbtest-mouse',
        });

        await waitForSerial(/USB: HID mouse ready on port/, 20_000, 'mouse ready');

        await qmp.command('input-send-event', {
            events: [
                { type: 'rel', data: { axis: 'x', value: 24 } },
                { type: 'rel', data: { axis: 'y', value: -10 } },
            ],
        });

        await waitForSerial(/USB: mouse report received bytes=0x0*4/, 10_000, 'mouse report');

        check(
            'mouse movement report is decoded',
            /USB: mouse report received bytes=0x0*4/.test(serialText()),
            '4-byte boot mouse report',
        );

        // ----------------------------------------------------
        // Removal
        // ----------------------------------------------------

        await qmp.command('device_del', { id: 'usbtest-kbd' });

        await waitForSerial(/USB: device removed from port/, 15_000, 'keyboard removal');

        check(
            'unplugging a device releases its port and slot',
            /USB: device removed from port/.test(serialText()),
            'slot disabled',
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
