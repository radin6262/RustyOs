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
## Commit Official #3
- removed the custom xHCI/USB HID input stack (kernel/src/usb, kernel/src/xhci_pci.rs, kernel/src/input/usb.rs) together with its USB DMA plumbing
- replaced it with a PS/2 (i8042) input driver: keyboard on IRQ1, mouse on IRQ12, IntelliMouse wheel support
- interrupts.rs now unmasks the PIC lines PS/2 needs (IRQ1, cascade IRQ2, IRQ12) and no longer reserves an xHCI MSI vector
