use std::io::{Read, Error as IOError, ErrorKind};
use std::fmt::{Formatter, Display, Error as FmtError};
use std::error::Error;

const NBBY: usize = 8; // Number of bits per byte
const MATCH_BITS: usize = 6;
const MATCH_MIN: usize = 3;
const MATCH_MAX: usize = ((1 << MATCH_BITS) + (MATCH_MIN - 1));
const OFFSET_MASK: usize = ((1 << (16 - MATCH_BITS)) - 1);
const LEMPEL_SIZE: usize = 1024;

pub struct LzjbEncoder<'a> {
    src: &'a [u8],
}

impl<'a> LzjbEncoder<'a> {
    fn new(src: &'a [u8]) -> LzjbEncoder<'a> {
        LzjbEncoder {
            src: src,
        }
    }
}

impl<'a> Read for LzjbEncoder<'a> {

    /// LZJB compress the bytes in `src` into `dst`
    fn read(&mut self, dst: &mut [u8]) -> Result<usize, IOError> {
        let mut src_i = 0; // Current index in src
        let mut dst_i = 0; // Current index in dst

        // We place 1 extra byte preceding every 8 bytes. Each bit in this byte is
        // a flag that corresponds to one of the 8 bytes that delimit it. If the
        // flag is set, the byte is a copy item. If the flag is 0, it is a literal
        // item. We'll call this the copy flag.

        // Stores the index of the current copy flag in dst
        let mut copymap = 0;

        // The current bit in the byte pointed at by `copymap`
        let mut copymask: usize = 1 << (NBBY - 1);

        // This is our cache
        let mut lempel = [0usize; LEMPEL_SIZE];

        while src_i < self.src.len() {
            copymask <<= 1;
            if copymask == (1 << NBBY) {
                // We've reached the end of our 8-byte cycle
                if dst_i >= dst.len() - 1 - 2 * NBBY {
                    // If we've reached the last two bytes, we're done
                    return Ok(self.src.len());
                }
                // Not done yet, reset the cycle
                copymask = 1;
                copymap = dst_i; // Point to our new copy flag byte
                dst[dst_i] = 0; // Place the new (initially clear) copy flag byte
                dst_i += 1;
            }

            if src_i > self.src.len() - MATCH_MAX {
                // Nearing the end of the data, don't bother searching for matches,
                // just copy.
                dst[dst_i] = self.src[src_i];
                src_i += 1;
                dst_i += 1;
                continue;
            }

            // Compute hash of current 3 byte slice. It will be the index to our
            // cache
            let mut hash = ((self.src[src_i] as usize) << 16) + ((self.src[src_i + 1] as usize) << 8) +
                (self.src[src_i + 2] as usize);
            hash += hash >> 9;
            hash += hash >> 5;
            let hp = (hash as usize) & (LEMPEL_SIZE - 1);

            // Look up the current 3 byte slice in the cache. We'll verify that it's
            // a valid entry later.
            let offset = (src_i - lempel[hp]) & OFFSET_MASK;
            let cpy = src_i - offset;

            // Set the current 3 byte slice as the most recent sighting of it in the
            // cache
            lempel[hp] = src_i;

            // Check that the cached item is valid
            if src_i >= offset && cpy != src_i && self.src[src_i] == self.src[cpy] &&
                self.src[src_i + 1] == self.src[cpy + 1] && self.src[src_i + 2] == self.src[cpy + 2] {
                    // This cache item is valid, write a copy item
                    dst[copymap] |= copymask as u8; // Set the

                    // Find the full length of this match. Since it was in the hash,
                    // we know the match length is at least 3.
                    let mut mlen = MATCH_MIN;
                    while mlen < MATCH_MAX {
                        if self.src[src_i + mlen] != self.src[cpy + mlen] {
                            break;
                        }
                        mlen += 1;
                    }

                    // Place the match length portion of the copy item
                    dst[dst_i] = (((mlen - MATCH_MIN) << (NBBY - MATCH_BITS)) | (offset >> NBBY)) as u8;
                    dst_i += 1;

                    // Place the offset portion of the copy item
                    dst[dst_i] = offset as u8;
                    dst_i += 1;

                    // Now we get to skip the repeated sequence!
                    src_i += mlen;
                } else {
                    // Not a real cache entry, don't make a copy item
                    dst[dst_i] = self.src[src_i];
                    dst_i += 1;
                    src_i += 1;
                }
        }

        Ok(dst_i)
    }
}

pub struct LzjbDecoder<'a> {
    src: &'a [u8],
}

impl<'a> LzjbDecoder<'a> {
    pub fn new(src: &'a [u8]) -> LzjbDecoder<'a> {
        LzjbDecoder {
            src: src,
        }
    }
}

#[derive(Debug)]
pub struct DecoderError;

impl Display for DecoderError {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        Ok(())
    }
}

impl Error for DecoderError {
    fn description(&self) -> &str {
        "Failed to decode. The data is likely corrupted."
    }

}


impl<'a> Read for LzjbDecoder<'a> {

    /// LZJB compress the bytes in `src` into `dst`
    fn read(&mut self, dst: &mut [u8]) -> Result<usize, IOError> {
        let mut src_i = 0;
        let mut dst_i = 0;
        let mut copymap: u8 = 0;
        let mut copymask: usize = 1 << (NBBY - 1);

        while dst_i < dst.len() {
            copymask <<= 1;
            if copymask == (1 << NBBY) {
                // Finished another 8-byte loop, repeat
                copymask = 1; // Reset the copy mask
                copymap = self.src[src_i]; // Current byte is the new copymap
                src_i += 1;
            }
            if (copymap & (copymask as u8)) != 0 {
                // Found a copy item
                let mlen = ((self.src[src_i] as usize) >> (NBBY - MATCH_BITS)) + MATCH_MIN;
                let offset = (((self.src[src_i] as usize) << NBBY) | (self.src[src_i + 1] as usize)) &
                    OFFSET_MASK;
                src_i += 2;
                if dst_i < offset {
                    // Copy item points to invalid index, error
                    return Err(IOError::new(ErrorKind::Other, DecoderError));
                }
                let mut cpy = dst_i - offset;
                for _ in 0..mlen {
                    if dst_i >= dst.len() {
                        // Reached the end of the destination buffer, can't copy anymore
                        break;
                    }
                    dst[dst_i] = dst[cpy];
                    dst_i += 1;
                    cpy += 1;
                }
            } else {
                // It's a literal item, copy it directly
                dst[dst_i] = self.src[src_i];
                dst_i += 1;
                src_i += 1;
            }
        }
        Ok(dst.len())
    }
}
