# RustyOS

RustyOS is a lightweight hobby operating system written in Rust.

> This project has been temporarily discotninued until i get some free time or next summer
> Help Wanted: if you know how to fix it(or my issue) please contact me at radindavari054laptop@gmail.com or open a GitHub issue

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
- Basic userspace window creation and rendering
- XHCI driver support for real hardware

## Source
Uses nightly Rust and the `bootloader` crate to create a UEFI bootable image. The kernel is written in Rust and uses the `x86_64` crate for low-level architecture-specific functionality.

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details
