## Commit Official #1
- Deprecated old WM and Main
- Added new WM and Main
- update syscalls
- split some apps into the programs/ folder
- add a test app to test ui via syscalls
- WASM compiler will be removed and deprecated soon, as it is beginning to be replaced with ELF
## Commit Official #2
- move deprecated memory file from src to kernel/src/memory.deprecated
- this deprecated memory file will be removed in future updates when current memory handler will be deemed stable
## Commit Official #4
- window pixel buffers are now page aligned, physically contiguous shared memory mapped into the owning process (SYS_WINDOW_BUFFER), verified with a page-table walk in the kernel
- RustySDK windows draw rectangles by writing that buffer instead of calling SYS_FILL_RECT/SYS_DRAW_RECT; only text still goes through the kernel (SYS_DRAW_STRING) because the font rasterizer lives there
- new SYS_SET_WINDOW_TITLE so applications can title their window
- compositor chrome rewritten macOS style: gradient wallpaper, translucent menu bar, rounded window frames with drop shadow and traffic lights, and a dock with hover labels pinned to the right edge
- tests/shm-test.mjs boots the image headlessly and checks the desktop through serial assertions and QEMU screendumps

## Commit Official #3
- removed the custom xHCI/USB HID input stack (kernel/src/usb, kernel/src/xhci_pci.rs, kernel/src/input/usb.rs) together with its USB DMA plumbing
- replaced it with a PS/2 (i8042) input driver: keyboard on IRQ1, mouse on IRQ12, IntelliMouse wheel support
- interrupts.rs now unmasks the PIC lines PS/2 needs (IRQ1, cascade IRQ2, IRQ12) and no longer reserves an xHCI MSI vector
