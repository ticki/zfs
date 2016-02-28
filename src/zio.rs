use std::{mem, ptr};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

use super::avl;
use super::block_ptr::BlockPtr;
use super::dvaddr::DVAddr;
use super::from_bytes::FromBytes;
use super::lzjb;
use super::uberblock::Uberblock;
use super::zfs;

pub const NUM_TYPES: usize = 6;
pub const NUM_TASKQ_TYPES: usize = 4;

pub struct Reader {
    pub disk: File,
}

impl Reader {
    // TODO: Error handling
    pub fn read(&mut self, start: usize, length: usize) -> Vec<u8> {
        let mut ret: Vec<u8> = vec![0; length*512];

        self.disk.seek(SeekFrom::Start(start as u64 * 512));
        self.disk.read(&mut ret);

        ret
    }

    pub fn write(&mut self, block: usize, data: &[u8; 512]) {
        self.disk.seek(SeekFrom::Start(block as u64 * 512));
        self.disk.write(data);
    }

    pub fn read_dva(&mut self, dva: &DVAddr) -> Vec<u8> {
        self.read(dva.sector() as usize, dva.asize() as usize)
    }

    pub fn read_block(&mut self, block_ptr: &BlockPtr) -> Result<Vec<u8>, &'static str> {
        let data = self.read_dva(&block_ptr.dvas[0]);
        match block_ptr.compression() {
            2 => {
                // compression off
                Ok(data)
            }
            1 | 3 => {
                // lzjb compression
                let mut decompressed = vec![0; (block_ptr.lsize()*512) as usize];
                lzjb::LzjbDecoder::new(&data).read(&mut decompressed);
                Ok(decompressed)
            }
            _ => Err("Error: not enough bytes"),
        }
    }

    /*
    pub fn read_type<T: FromBytes>(&mut self, block_ptr: &BlockPtr) -> Result<T, &'static str> {
        self.read_block(block_ptr).and_then(|data| T::from_bytes(&data[..]))
    }
    */

    pub fn read_type_array<T: FromBytes>(&mut self,
                                         block_ptr: &BlockPtr,
                                         offset: usize)
        -> Result<T, String> {
            self.read_block(block_ptr).map_err(|x| x.to_owned()).and_then(|data| T::from_bytes(&data[offset * mem::size_of::<T>()..]).map_err(|x| x.to_owned()))
        }

    pub fn uber(&mut self) -> Result<Uberblock, &'static str> {
        let mut newest_uberblock: Option<Uberblock> = None;
        for i in 0..128 {
            if let Ok(uberblock) = Uberblock::from_bytes(&self.read(256 + i * 2, 2)) {
                let newest = match newest_uberblock {
                    Some(previous) => {
                        if uberblock.txg > previous.txg {
                            // Found a newer uberblock
                            true
                        } else {
                            false
                        }
                    }
                    // No uberblock yet, so first one we find is the newest
                    None => true,
                };

                if newest {
                    newest_uberblock = Some(uberblock);
                }
            }
        }

        match newest_uberblock {
            Some(uberblock) => Ok(uberblock),
            None => Err("Failed to find valid uberblock"),
        }
    }
}

/// ZIOO priority
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Priority {
    /// Non-queued IO
    Now,
    /// Synchronous read
    SyncRead,
    /// Synchronous write
    SyncWrite,
    /// Log write
    LogWrite,
    /// Cache fills
    CacheFill,
    /// Deduplication table prefetch
    DTP,
    /// Free
    Free,
    /// Asynchronous read
    AsyncRead,
    /// Asynchronous write
    AsyncWrite,
    /// Resilver
    Resilver,
    /// Scrub
    Scrub,
}

/// ZIO task
#[derive(Copy, Clone, PartialEq)]
pub enum Type {
    /// Nothin'
    Null,
    /// Read
    Read,
    /// Write
    Write,
    /// Free data
    Free,
    /// Claim data
    Claim,
    /// IO control (VDev modifications etc.)
    IoCtl,
}
enum Stage {
    /// RWFCI
    Open = 1 << 0,
    /// R....
    ReadBpInit = 1 << 1,
    /// ..F..
    FreeBpInit = 1 << 2,
    /// RWF..
    IssueAsync = 1 << 3,
    /// .W...
    WriteBpInit = 1 << 4,
    /// .W...
    ChecksumGenerate = 1 << 5,
    /// .W...
    NopWrite = 1 << 6,
    /// R....
    DdtReadStart = 1 << 7,
    /// R....
    DdtReadDone = 1 << 8,
    /// .W...
    DdtWrite = 1 << 9,
    /// ..F..
    DdtFree = 1 << 10,
    /// RWFC.
    GangAssemble = 1 << 11,
    /// RWFC.
    GangIssue = 1 << 12,
    /// .W...
    DvaAllocate = 1 << 13,
    /// ..F..
    DvaFree = 1 << 14,
    /// ...C.
    DvaClaim = 1 << 15,
    /// RWFCI
    Ready = 1 << 16,
    /// RW..I
    VdevIoStart = 1 << 17,
    /// RW..I
    VdevIoDone = 1 << 18,
    /// RW..I
    VdevIoAssess = 1 << 19,
    /// R....
    ChecksumVerify = 1 << 20,
    /// RWFCI
    Done = 1 << 21,
}

/// Taskq type
pub enum TaskqType {
    /// An "issue"
    Issue,
    /// An high-priority "issue"
    IssueHigh,
    /// Interrupt
    Interrupt,
    /// High-priority interrupt
    InterruptHigh,
}

#[derive(Copy, Clone, PartialEq)]
enum PipelineFlow {
    Continue = 0x100,
    Stop = 0x101,
}

#[derive(Copy, Clone, PartialEq)]
enum Flag {
    /// Must be equal for two zios to aggregate
    DontAggregate  = 1 << 0,
    IoRepair       = 1 << 1,
    SelfHeal       = 1 << 2,
    Resilver       = 1 << 3,
    Scrub          = 1 << 4,
    ScanThread     = 1 << 5,
    Physical       = 1 << 6,

    /// Inherited by ddt, gang, or vdev children.
    CanFail        = 1 << 7,
    Speculative    = 1 << 8,
    ConfigWriter   = 1 << 9,
    DontRetry      = 1 << 10,
    DontCache      = 1 << 11,
    NoData         = 1 << 12,
    InduceDamage   = 1 << 13,

    /// Vdev inherited (from children)
    IoRetry        = 1 << 14,
    Probe          = 1 << 15,
    TryHard        = 1 << 16,
    Optional       = 1 << 17,

    /// Non-inherited
    DontQueue      = 1 << 18,
    DontPropagate  = 1 << 19,
    IoBypass       = 1 << 20,
    IoRewrite      = 1 << 21,
    Raw            = 1 << 22,
    GangChild      = 1 << 23,
    DdtChild       = 1 << 24,
    GodFather      = 1 << 25,
    NopWrite       = 1 << 26,
    ReExecuted     = 1 << 27,
    Delegated      = 1 << 28,
    FastWrite      = 1 << 29,
}

#[derive(Copy, Clone, PartialEq)]
enum Child {
    Vdev = 0,
    Gang,
    Ddt,
    Logical,
}

#[repr(u8)]
enum WaitType {
    Ready = 0,
    Done,
}
