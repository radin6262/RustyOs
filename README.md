# RustyOS

RustyOS is a lightweight hobby operating system written in Rust.

a simple modern uefi hobby operating system

## Version Logs
Version logs will contain changlogs and deprecation notices. [Click Me](versionlogs.md) to go version logs

## Features

- UEFI boot
- x86_64 kernel
- Custom memory management
- GDT and IDT
- Interrupt handling
- Framebuffer graphics
- Window manager and compositor
- Double-buffered rendering
- USB keyboard support through xHCI(xhci-nostd)
- Ring 3 userspace
- Separate process address spaces
- CR3 context switching
- Cooperative process scheduler
- System calls
- WASM application runtime
- Basic userspace window creation and rendering

## Source
Uses nightly Rust and the `bootloader` crate to create a UEFI bootable image. The kernel is written in Rust and uses the `x86_64` crate for low-level architecture-specific functionality.

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details