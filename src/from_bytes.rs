use std::{mem, ptr};

pub trait FromBytes: Sized {
    fn from_bytes(data: &[u8]) -> Result<Self, &str> {
        if data.len() >= mem::size_of::<Self>() {
            let s = unsafe { ptr::read(data.as_ptr() as *const Self) };
            Ok(s)
        } else {
            Err("Buffer not long enough.")
        }
    }
}

impl FromBytes for u64 {}
