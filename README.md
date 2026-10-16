# RustyOS

RustyOS is a lightweight hobby operating system written in Rust.

> Warning as of now only laptop keyboards and PS/2 devices are supported. Touchpads are not supported

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

## Input

Input comes from the legacy PS/2 (i8042) controller: IRQ1 delivers keyboard
scancodes and IRQ12 delivers mouse packets, both decoded into the kernel's
`InputEvent` queue. Every PC and every QEMU machine provides this controller by
default, so the OS has working input on real hardware without any USB stack.

Keyboard: US layout, shift/ctrl/alt/gui and caps/num/scroll lock, arrow keys,
and the numeric keypad (digits when num lock is on, arrow keys otherwise).
Mouse: three-button movement plus the wheel on IntelliMouse-compatible devices.

## Building and running

`run.ps1` builds the kernel and boots the UEFI image in QEMU.

`makeusb.ps1` builds the same image and writes it to a USB drive for real
hardware:

```
.\makeusb.ps1            # build the image and save .\rusty-usb.img
.\makeusb.ps1 -Drive 1   # build the image and flash physical drive 1
```

## Source
Uses nightly Rust and the `bootloader` crate to create a UEFI bootable image. The kernel is written in Rust and uses the `x86_64` crate for low-level architecture-specific functionality.

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details
