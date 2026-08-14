//! Memory pressure signal definitions shared across cores.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressureLevel {
    Normal = 0,
    Moderate = 1,
    Critical = 2,
}

impl MemoryPressureLevel {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Moderate,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }
}
