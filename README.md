# RustyOS

RustyOS is a lightweight hobby operating system written in Rust.

> Warning as of now keyboards and mice that expose a HID boot interface are
> supported over both USB and PS/2. Touchpads and devices that only speak a
> custom HID report format are not supported.


## Features

- UEFI boot
- x86_64 kernel
- Custom memory management
- GDT and IDT
- Interrupt handling
- Framebuffer graphics
- Window manager and compositor
- Double-buffered rendering
- Shared memory window buffers: applications draw into their window without a
  syscall per rectangle
- macOS style desktop chrome: menu bar, window title bars with traffic lights,
  rounded window corners and a dock pinned to the right edge
- PS/2 keyboard and mouse input (i8042, IRQ1/IRQ12)
- xHCI USB host controller driver: every controller on the PCI bus is driven
  independently, with controller reset, command/event rings, scratchpad
  buffers, USB2/USB3 root ports, device slots, control and interrupt transfers
- USB HID boot keyboards and mice, with hotplug on root ports
- USB 2.0 hub support, including nested hubs, route-string addressing and
  transaction-translator fields for low/full-speed devices behind HS hubs
- HID report-descriptor parsing for non-boot keyboards, mice and tablets,
  including absolute pointing devices
- Composite HID devices: every HID interface on a device is configured and
  polled independently
- PS/2 and USB input share one event queue, so either backend drives the
  compositor and applications
- Ring 3 userspace
- Separate process address spaces
- CR3 context switching
- Cooperative process scheduler
- System calls
- Basic userspace window creation and rendering

## Desktop

The compositor draws a macOS style desktop: a gradient wallpaper, a
translucent menu bar with the focused window's title, rounded windows with a
drop shadow and a title bar carrying the traffic lights, and a translucent dock
on the right edge with hover labels.

Applications draw into their window's pixel buffer, which the kernel maps into
their address space (`SYS_WINDOW_BUFFER`). Rectangle drawing is therefore plain
memory access; only text still goes through the kernel, because the font
rasterizer lives there.

## Tests

`tests/shm-test.mjs` boots the built UEFI image headlessly in QEMU and checks
the desktop through serial log assertions and screendumps:

```
node tests/shm-test.mjs          # run every check
node tests/shm-test.mjs --keep   # keep the screendumps
```

It verifies the shared window mapping (including a kernel page-table walk), that
applications no longer use the per-rectangle syscalls while their window content
still reaches the screen, the chrome (menu bar, traffic lights, rounded corners,
dock and hover label) and that typing reaches the application.

`tests/screendump.mjs` converts a screendump into a PNG for inspection:

```
node tests/screendump.mjs target/shm-test/before.ppm screen.png
```

`tests/usb-test.mjs` boots with two `qemu-xhci` controllers and no USB
devices, then hotplugs a keyboard and a mouse over QMP:

```
node tests/usb-test.mjs
```

It checks both controller resets, the USB2/USB3 root-port split, the
scratchpad setup path, boot-keyboard and mouse enumeration, key/movement
report decoding, and that unplugging a device releases its slot.

`tests/usb-hub-test.mjs` boots with two nested USB 2.0 hubs and a keyboard on
the second hub's first port:

```
node tests/usb-hub-test.mjs
```

It checks both hub descriptors, the nested route strings (`0x1` and `0x11`)
and keyboard input through the whole tree.

`tests/usb-hid-test.mjs` attaches a USB tablet, which has no HID boot
interface, and verifies the report-descriptor path:

```
node tests/usb-hid-test.mjs
```

It checks that the tablet is classified as a pointing device, that its
6-byte report is decoded, and that its absolute coordinates move the
compositor cursor to the scaled position on screen.

## Input

Two backends feed the same `InputEvent` queue:

- PS/2 (i8042): IRQ1 carries keyboard scancodes and IRQ12 mouse packets.
- USB: the kernel's own xHCI driver polls its event ring from the syscall
  entry path. Every xHCI controller found on the PCI bus is brought up and
  driven independently. Each controller is reset, its USB2 and USB3 root
  ports are walked, and HID keyboards, mice and tablets are driven with
  interrupt IN transfers. Scratchpad buffers are allocated when HCSPARAMS2
  requires them. USB 2.0 hubs are enumerated recursively, including the
  route strings and transaction translator fields needed for devices behind
  them. Port status change events trigger a rescan, so devices can be plugged
  in after boot.

Devices that expose a HID boot interface use the fixed boot reports. Other
HID interfaces are decoded from their report descriptor; this covers
keyboards, relative mice and absolute tablets. A device may expose several
HID interfaces and each one is configured independently.

USB 3.x hubs are not supported. Devices already present behind a hub are
enumerated when the hub is attached, but plugging a device into a hub after
boot is not polled yet.

Keyboard: US layout, shift/ctrl/alt/gui and caps/num/scroll lock, arrow keys,
and the numeric keypad (digits when num lock is on, arrow keys otherwise).
Mouse: three-button movement plus the wheel when the device reports it.

## Building and running

`run.ps1` builds the kernel and boots the UEFI image in QEMU with a `qemu-xhci`
controller and a USB keyboard and mouse attached. Remove the `qemu-xhci`,
`usb-kbd` and `usb-mouse` flags to run against PS/2 instead; both backends stay
compiled in.

`makeusb.ps1` builds the same image and writes it to a USB drive for real
hardware:

```
.\makeusb.ps1            # build the image and save .\rusty-usb.img
.\makeusb.ps1 -Drive 1   # build the image and flash physical drive 1
```

### Windows build note

Some recent Windows Defender updates deny `link.exe` access to its temporary
files under `%TEMP%`, which breaks the bootloader's nested build with
`LNK1104: cannot open file ...lnk{GUID}.tmp`. `.cargo/config.toml` therefore
points the temporary directory of build scripts and the bootloader's nested
`cargo install` at `target/`. If you prefer to keep the system temp directory,
exclude the project and `CARGO_HOME` from Defender instead.

## Source
Uses nightly Rust and the `bootloader` crate to create a UEFI bootable image. The kernel is written in Rust and uses the `x86_64` crate for low-level architecture-specific functionality.

`kernel/src/usb` contains the xHCI host controller driver (PCI discovery,
controller reset, command/event/transfer rings, scratchpads, root ports,
device slots and standard requests), the USB 2.0 hub class driver, and the
HID layer with both boot-protocol and report-descriptor decoders. It is
implemented from scratch against the Intel xHCI and USB HID specifications
and does not use external USB crates.

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details
