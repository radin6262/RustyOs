// ============================================================
// RustyOS USB hub enumeration test
// ============================================================
//
// Boots with two nested QEMU USB 2.0 hubs and a boot keyboard on the second
// hub's first port (QEMU port path 1.1.1). It checks that the driver:
//
//   1. enumerates the first hub on the root port
//   2. recurses into the second hub with route string 0x1
//   3. addresses the keyboard at route string 0x11
//   4. decodes input from the device behind both hubs
//
// Usage:
//
//     node tests/usb-hub-test.mjs
//

import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';
import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';

const REPO = path.resolve(import.meta.dirname, '..');

const QEMU = 'C:\\Program Files\\qemu\\qemu-system-x86_64.exe';
const BIOS = 'C:\\Program Files\\qemu\\OVMF.fd';

const OUT_DIR = path.join(REPO, 'target', 'usb-hub-test');
const SERIAL_LOG = path.join(OUT_DIR, 'serial.log');

const QMP_PORT = 45461;

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

    console.log('RustyOS USB hub test');
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
            '-device', 'usb-hub,id=hub0,bus=xhci.0',
            '-device', 'usb-hub,id=hub1,bus=xhci.0,port=1.1',
            '-device', 'usb-kbd,bus=xhci.0,port=1.1.1',
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
        await waitForSerial(/USB: HID keyboard ready on port/, 120_000, 'hub keyboard');

        const log = serialText();

        const hubs = (log.match(/USB: hub slot=/g) ?? []).length;

        check('both hubs enumerate', hubs === 2, `${hubs} hubs reported`);

        check(
            'first hub is on the root port',
            /USB: hub slot=.* ports=0x0*8/.test(log),
            '8 downstream ports',
        );

        check(
            'second hub is discovered at route 0x1',
            /hub port .*route=0x0*1[\r\n]/.test(log),
            'nested hub route',
        );

        check(
            'keyboard is found at route 0x11 behind both hubs',
            /HID keyboard ready on port .*route=0x0*11/.test(log),
            'two-level route string',
        );

        qmp = await Qmp.connect(QMP_PORT);

        await qmp.command('qmp_capabilities');

        await qmp.command('send-key', {
            keys: [{ type: 'qcode', data: 'q' }],
            'hold-time': 100,
        });

        await waitForSerial(/USB: first key usage=0x0*14/, 10_000, 'key behind hubs');

        check(
            'keyboard input works through both hubs',
            /USB: first key usage=0x0*14/.test(serialText()),
            'usage 0x14 (q) decoded',
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
