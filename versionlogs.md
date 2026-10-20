## Commit Official #5
- replaced the removed USB stack with a from-scratch xHCI driver: PCI discovery,
  BAR sizing and MMIO mapping, firmware handoff, halt/reset, DCBAA, command,
  event and transfer rings, USB2/USB3 root-port reset, Enable Slot, Address
  Device, Configure Endpoint and synchronous control transfers
- every xHCI controller on the PCI bus is driven independently; devices, slots
  and hotplug state are kept per controller
- scratchpad buffers are allocated from the DMA region and published through
  DCBAA[0] when HCSPARAMS2 reports a nonzero SPB count
- USB HID boot keyboard and mouse support: configuration descriptor parsing,
  SET_CONFIGURATION/SET_PROTOCOL/SET_IDLE, one interrupt IN transfer per device,
  boot report decoding into the existing `InputEvent` queue
- hotplug on root ports: port status change events trigger a rescan, enumerate a
  new device, or release its slot when it is unplugged
- USB is polled from the syscall entry path next to the compositor tick; PS/2
  input stays compiled in as a fallback
- tests/usb-test.mjs boots with two qemu-xhci controllers and hotplugs a
  keyboard and mouse over QMP; run.ps1 now attaches usb-kbd/usb-mouse
- external USB 2.0 hub support: hub descriptor parsing, per-port power and
  reset, route-string addressing and transaction-translator fields for
  low/full-speed devices behind high-speed hubs, including nested hubs
- two-phase Address Device (BSR=1, read descriptors, BSR=0) so the Hub bit and
  the real EP0 max packet are known before the final address is assigned
- HID report-descriptor parser for non-boot devices: keyboard modifier/key
  arrays, mouse buttons and relative axes, absolute tablets with scaling onto
  the framebuffer, and report-ID support
- composite HID devices: every HID interface in a configuration is configured
  and polled independently, sharing the device slot
- new tests: tests/usb-hub-test.mjs (nested hubs, routes 0x1 and 0x11) and
  tests/usb-hid-test.mjs (tablet report descriptor and absolute cursor)


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
- remove deprecated files

## Commit Official #2
- move deprecated memory file from src to kernel/src/memory.deprecated
- this deprecated memory file will be removed in future updates when current memory handler will be deemed stable

## Commit Official #1
- Deprecated old WM and Main
- Added new WM and Main
- update syscalls
- split some apps into the programs/ folder
- add a test app to test ui via syscalls
- WASM compiler will be removed and deprecated soon, as it is beginning to be replaced with ELF
