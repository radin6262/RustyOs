use wasmi::{
    Caller,
    Engine,
    Extern,
    Func,
    Instance,
    Module,
    Store,
};

use crate::serial;

// ============================================================
// Minimal Rusty WASM ABI
// ============================================================
//
// WASM imports:
//
//     (import "rusty" "write"
//         (func $write (param i32 i32)))
//
// Parameters:
//
//     ptr = pointer into WASM linear memory
//     len = number of bytes
//
// ============================================================

fn host_write(
    caller: Caller<'_, ()>,
    ptr: i32,
    len: i32,
) {
    if ptr < 0 || len < 0 {
        serial::write_str(
            "wasm: invalid write arguments\n",
        );

        return;
    }

    let memory =
        match caller.get_export("memory") {
            Some(Extern::Memory(memory)) => memory,

            _ => {
                serial::write_str(
                    "wasm: module has no exported memory\n",
                );

                return;
            }
        };

    let data =
        memory.data(&caller);

    let start =
        ptr as usize;

    let length =
        len as usize;

    let end =
        match start.checked_add(length) {
            Some(end) => end,

            None => {
                serial::write_str(
                    "wasm: write range overflow\n",
                );

                return;
            }
        };

    if end > data.len() {
        serial::write_str(
            "wasm: write outside linear memory\n",
        );

        return;
    }

    match core::str::from_utf8(
        &data[start..end],
    ) {
        Ok(string) => {
            serial::write_str(
                string,
            );
        }

        Err(_) => {
            serial::write_str(
                "wasm: invalid UTF-8\n",
            );
        }
    }
}

// ============================================================
// Run a WASM module
// ============================================================

pub fn run(
    wasm: &[u8],
) -> Result<(), wasmi::Error> {
    serial::write_str(
        "wasm: creating engine\n",
    );

    let engine =
        Engine::default();

    serial::write_str(
        "wasm: loading module\n",
    );

    let module =
        match Module::new(
            &engine,
            wasm,
        ) {
            Ok(module) => {
                serial::write_str(
                    "wasm: module loaded\n",
                );

                module
            }

            Err(error) => {
                serial::write_str(
                    "wasm: module load FAILED\n",
                );

                return Err(error);
            }
        };

    serial::write_str(
        "wasm: creating store\n",
    );

    let mut store =
        Store::new(
            &engine,
            (),
        );

    serial::write_str(
        "wasm: creating host function\n",
    );

    let write_func =
        Func::wrap(
            &mut store,
            host_write,
        );

    serial::write_str(
        "wasm: creating instance\n",
    );

    let instance =
        match Instance::new(
            &mut store,
            &module,
            &[write_func.into()],
        ) {
            Ok(instance) => {
                serial::write_str(
                    "wasm: instance created\n",
                );

                instance
            }

            Err(error) => {
                serial::write_str(
                    "wasm: instance creation FAILED\n",
                );

                return Err(error);
            }
        };

    serial::write_str(
        "wasm: looking up main\n",
    );

    let main =
        match instance
            .get_typed_func::<(), ()>(
                &store,
                "main",
            )
        {
            Ok(main) => {
                serial::write_str(
                    "wasm: main found\n",
                );

                main
            }

            Err(error) => {
                serial::write_str(
                    "wasm: main lookup FAILED\n",
                );

                return Err(error);
            }
        };

    serial::write_str(
        "wasm: calling main\n",
    );

    match main.call(
        &mut store,
        (),
    ) {
        Ok(()) => {
            serial::write_str(
                "wasm: main returned\n",
            );

            Ok(())
        }

        Err(error) => {
            serial::write_str(
                "wasm: main execution FAILED\n",
            );

            Err(error)
        }
    }
}

// ============================================================
// Temporary embedded WASM application
// ============================================================
//
// Equivalent WAT:
//
// (module
//
//   (import "rusty" "write"
//       (func $write
//           (param i32 i32)))
//
//   (memory (export "memory") 1)
//
//   (data (i32.const 0)
//       "Hello from WASM!\n")
//
//   (func (export "main")
//       i32.const 0
//       i32.const 17
//       call $write)
// )
//
// ============================================================

static HELLO_WASM: &[u8] = &[
    // WASM magic & version
    0x00, 0x61, 0x73, 0x6D,
    0x01, 0x00, 0x00, 0x00,

    // --------------------------------------------------------
    // Type section (Fixed length: 0x09)
    //
    // Type 0: () -> ()
    // Type 1: (i32, i32) -> ()
    // --------------------------------------------------------
    0x01, 0x09, // section length: 9 bytes
    0x02,       // 2 types
    // Type 0: () -> ()
    0x60, 0x00, 0x00,
    // Type 1: (i32, i32) -> ()
    0x60, 0x02, 0x7F, 0x7F, 0x00,

    // --------------------------------------------------------
    // Import section
    // --------------------------------------------------------
    0x02, 0x0F,
    0x01,
    0x05, 0x72, 0x75, 0x73, 0x74, 0x79, // "rusty"
    0x05, 0x77, 0x72, 0x69, 0x74, 0x65, // "write"
    0x00, // import kind = function
    0x01, // type index = 1 ((i32, i32) -> ())

    // --------------------------------------------------------
    // Function section
    // --------------------------------------------------------
    0x03, 0x02,
    0x01,
    0x00, // type index = 0 (() -> ())

    // --------------------------------------------------------
    // Memory section
    // --------------------------------------------------------
    0x05, 0x03,
    0x01,
    0x00, 0x01,

    // --------------------------------------------------------
    // Export section
    // --------------------------------------------------------
    0x07, 0x11,
    0x02,
    0x04, 0x6D, 0x61, 0x69, 0x6E, 0x00, 0x01, // "main" -> func 1
    0x06, 0x6D, 0x65, 0x6D, 0x6F, 0x72, 0x79, 0x02, 0x00, // "memory" -> mem 0

    // --------------------------------------------------------
    // Code section
    // --------------------------------------------------------
    0x0A, 0x0A,
    0x01,
    0x08, // body size
    0x00, // no locals
    0x41, 0x00, // i32.const 0
    0x41, 0x11, // i32.const 17
    0x10, 0x00, // call imported function 0 (rusty.write)
    0x0B, // end

    // --------------------------------------------------------
    // Data section
    // --------------------------------------------------------
    0x0B, 0x17,
    0x01,
    0x00,
    0x41, 0x00,
    0x0B,
    0x11,
    0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x66, 0x72, 0x6F, 0x6D, 0x20, 0x57, 0x41, 0x53, 0x4D, 0x21, 0x0A,
];

// ============================================================
// Demo
// ============================================================

pub fn run_demo() {
    serial::write_str(
        "\n=== Rusty WASM Demo ===\n",
    );

    match run(
        HELLO_WASM,
    ) {
        Ok(()) => {
            serial::write_str(
                "wasm: application completed successfully\n",
            );
        }

        Err(_) => {
            serial::write_str(
                "wasm: application failed\n",
            );

            serial::write_str(
                "wasm: runtime returned an error\n",
            );
        }
    }

    serial::write_str(
        "=== End WASM Demo ===\n\n",
    );
}