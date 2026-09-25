#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotContext {
    pub dword: [u32; 8],
}

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Default)]
pub struct EndpointContext {
    pub dword: [u32; 8],
}

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct DeviceContext {
    pub slot: SlotContext,
    pub endpoints: [EndpointContext; 31],
}

impl Default for DeviceContext {
    fn default() -> Self {
        Self {
            slot: SlotContext::default(),
            endpoints: [EndpointContext::default(); 31],
        }
    }
}

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct InputContext {
    pub drop_flags: u32,
    pub add_flags: u32,
    pub reserved: [u32; 6],
    pub device: DeviceContext,
}

impl Default for InputContext {
    fn default() -> Self {
        Self {
            drop_flags: 0,
            add_flags: 0,
            reserved: [0; 6],
            device: DeviceContext::default(),
        }
    }
}
