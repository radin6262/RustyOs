pub mod boothandler;
pub mod test_ring3;

pub static Terminal_ELF: &[u8] =
    include_bytes!("../../../user/terminal/Terminal");