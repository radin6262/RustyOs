Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass


cargo clean


cargo build


$RustyImage = Get-ChildItem `
    "D:\RustProjects\Rusty\target\debug\build" `
    -Recurse `
    -Filter "rusty-uefi.img" |
    Select-Object -First 1 -ExpandProperty FullName

# Input comes from the USB stack: qemu-xhci is the controller Rusty drives,
# and usb-kbd/usb-mouse are attached to its root ports. Remove those three
# -device flags to run against the PS/2 controller instead (the kernel keeps
# both backends).
qemu-system-x86_64 `
    -machine q35 `
    -m 512M `
    -bios "C:\Program Files\qemu\OVMF.fd" `
    -device qemu-xhci,id=xhci `
    -device usb-kbd,bus=xhci.0 `
    -device usb-mouse,bus=xhci.0 `
    -drive "format=raw,file=$RustyImage" `
    -serial stdio
