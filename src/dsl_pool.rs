use super::spa;
use super::zfs;

pub struct DslPool {
    // Immutable
    root_dir_obj: u64,
    pub dp_dirty_total: u32,
}

impl DslPool {
    pub fn init(spa: &mut spa::Spa, txg: u64) -> zfs::Result<Self> {
        Self::open_impl(spa, txg)
    }

    fn open_impl(spa: &mut spa::Spa, txg: u64) -> zfs::Result<Self> {
        Ok(Self {
            root_dir_obj: 0,
            dp_dirty_total: 0,
        })
    }

    pub fn new() -> Self {
        Self {
            root_dir_obj: 0,
            dp_dirty_total: 0,
        }
    }
}
