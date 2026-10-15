Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass


cargo clean


cargo build


$RustyImage = Get-ChildItem `
    "D:\RustProjects\Rusty\target\debug\build" `
    -Recurse `
    -Filter "rusty-uefi.img" |
    Select-Object -First 1 -ExpandProperty FullName

# Input comes from QEMU's built-in PS/2 controller (i8042), which every
# pc/q35 machine provides. Do NOT attach usb-kbd/usb-mouse here: QEMU hands the
# host's keyboard and mouse to the newest HID handler, so USB devices would
# starve the PS/2 ports that Rusty now drives.
qemu-system-x86_64 `
    -machine q35 `
    -m 512M `
    -bios "C:\Program Files\qemu\OVMF.fd" `
    -drive "format=raw,file=$RustyImage" `
    -serial stdio
