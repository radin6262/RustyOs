# RustyOS

RustyOS is a lightweight hobby operating system written in Rust.

> Warning: Xhci driver only works on asus tuf f15 laptop due to xhci filtering(but soon we will initlize each controller and bring full device support to each pc and laptop)

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
