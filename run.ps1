cargo clean


cargo build


$RustyImage = Get-ChildItem `
    "D:\RustProjects\Rusty\target\debug\build" `
    -Recurse `
    -Filter "rusty-uefi.img" |
    Select-Object -First 1 -ExpandProperty FullName

qemu-system-x86_64 `
    -machine q35 `
    -m 512M `
    -bios "C:\Program Files\qemu\OVMF.fd" `
    -device qemu-xhci,id=xhci `
    -device usb-kbd,bus=xhci.0 `
    -device usb-mouse,bus=xhci.0 `
    -drive "format=raw,file=$RustyImage" `
    -serial stdio
