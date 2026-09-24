use std::env;
use std::path::PathBuf;

fn main() {
    let kernel = PathBuf::from(
        env::var_os("CARGO_BIN_FILE_KERNEL_kernel").expect("kernel artifact was not built"),
    );

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR was not set"));

    let uefi_image = out_dir.join("rusty-uefi.img");

    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&uefi_image)
        .expect("failed to create UEFI disk image");

    println!("cargo:rustc-env=RUSTY_UEFI_IMAGE={}", uefi_image.display());
}
