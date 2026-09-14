use x86_64::instructions::port::Port;

const COM1: u16 = 0x3F8;

pub fn init() {
    unsafe {
        let mut interrupt_enable: Port<u8> =
            Port::new(COM1 + 1);

        let mut line_control: Port<u8> =
            Port::new(COM1 + 3);

        let mut fifo_control: Port<u8> =
            Port::new(COM1 + 2);

        let mut modem_control: Port<u8> =
            Port::new(COM1 + 4);

        // Disable interrupts.
        interrupt_enable.write(0x00);

        // Enable DLAB.
        line_control.write(0x80);

        // 115200 baud.
        Port::<u8>::new(COM1).write(0x01);
        Port::<u8>::new(COM1 + 1).write(0x00);

        // 8 bits, no parity, one stop bit.
        line_control.write(0x03);

        // Enable FIFO, clear them, 14-byte threshold.
        fifo_control.write(0xC7);

        // IRQs enabled, RTS/DSR set.
        modem_control.write(0x0B);
    }
}

fn is_transmit_empty() -> bool {
    unsafe {
        let mut line_status: Port<u8> =
            Port::new(COM1 + 5);

        (line_status.read() & 0x20) != 0
    }
}

pub fn write_byte(byte: u8) {
    while !is_transmit_empty() {
        core::hint::spin_loop();
    }

    unsafe {
        let mut data: Port<u8> =
            Port::new(COM1);

        data.write(byte);
    }
}

pub fn write_str(string: &str) {
    for byte in string.bytes() {
        if byte == b'\n' {
            write_byte(b'\r');
        }

        write_byte(byte);
    }
}

pub fn write_hex(value: u64) {
    const HEX: &[u8; 16] =
        b"0123456789ABCDEF";

    write_str("0x");

    for shift in (0..64).step_by(4).rev() {
        let digit =
            ((value >> shift) & 0xF) as usize;

        write_byte(
            HEX[digit],
        );
    }
}

pub fn write_usize(value: usize) {
    write_hex(value as u64);
}