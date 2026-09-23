# RustyOS

> This Project is discontinued. i honestly rage quit because i absouletely could not make the xhci driver work

RustyOS is a lightweight hobby operating system written in Rust.


<!--## Version Logs
Version logs will contain changlogs and deprecation notices. [Click Me](versionlogs.md) to go version logs-->

<!--## Features

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
- Basic userspace window creation and rendering-->

## Source
Uses nightly Rust and the `bootloader` crate to create a UEFI bootable image. The kernel is written in Rust and uses the `x86_64` crate for low-level architecture-specific functionality.

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details
