#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::io::{self, Write};

use anyhow::{ensure, Result};
use crc::{Crc, Digest, CRC_32_ISO_HDLC};

////////////////////////////////////////////////////////////////////////////////

const HISTORY_SIZE: usize = 32768;
const CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

pub struct TrackingWriter<'a, T> {
    inner: T,
    history: VecDeque<u8>,
    byte_count: usize,
    crc32: Digest<'a, u32>,
}

impl<'a, T: Write> Write for TrackingWriter<'a, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.crc32.update(&buf[..written]);
        for &byte in buf[..written].iter() {
            if self.history.len() >= HISTORY_SIZE {
                self.history.pop_front();
            }
            self.history.push_back(byte);
        }
        self.byte_count += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush().unwrap();
        self.byte_count = 0;
        self.history = VecDeque::with_capacity(HISTORY_SIZE);
        self.crc32 = CRC.digest();
        Ok(())
    }
}

impl<'a, T: Write> TrackingWriter<'a, T> {
    pub fn new(inner: T) -> Self {
        Self {
            byte_count: 0,
            history: VecDeque::with_capacity(HISTORY_SIZE),
            crc32: CRC.digest(),
            inner,
        }
    }

    /// Write a sequence of `len` bytes written `dist` bytes ago.
    pub fn write_previous(&mut self, dist: usize, len: usize) -> Result<()> {
        ensure!(dist <= self.history.len(), "dist is out of border");
        ensure!(dist < HISTORY_SIZE, "dist must be less {}", HISTORY_SIZE);
        let mut result = Vec::with_capacity(len);

        self.history.make_contiguous();
        let start = self.history.len() - dist;
        let data = self.history.as_slices().0;

        let mut ind = start;
        for _ in 0..len {
            result.push(data[ind]);
            ind = if ind == std::cmp::min(data.len(), start + len) - 1 {
                start
            } else {
                ind + 1
            }
        }
        ensure!(self.write(&result)? == len, "could not write fully");
        Ok(())
    }

    pub fn byte_count(&self) -> usize {
        self.byte_count
    }

    pub fn crc32(&mut self) -> u32 {
        self.crc32.clone().finalize()
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;

    #[test]
    fn write() -> Result<()> {
        let mut buf: &mut [u8] = &mut [0u8; 10];
        let mut writer = TrackingWriter::new(&mut buf);

        assert_eq!(writer.write(&[1, 2, 3, 4])?, 4);
        assert_eq!(writer.byte_count(), 4);

        assert_eq!(writer.write(&[4, 8, 15, 16, 23])?, 5);
        assert_eq!(writer.byte_count(), 9);

        assert_eq!(writer.write(&[0, 0, 123])?, 1);
        assert_eq!(writer.byte_count(), 10);

        assert_eq!(writer.write(&[42, 124, 234, 27])?, 0);
        assert_eq!(writer.byte_count(), 10);
        assert_eq!(writer.crc32(), 2992191065);

        Ok(())
    }

    #[test]
    fn write_previous() -> Result<()> {
        let mut buf: &mut [u8] = &mut [0u8; 512];
        let mut writer = TrackingWriter::new(&mut buf);

        for i in 0..=255 {
            writer.write_u8(i)?;
        }

        writer.write_previous(192, 128)?;
        assert_eq!(writer.byte_count(), 384);

        assert!(writer.write_previous(10000, 20).is_err());
        assert_eq!(writer.byte_count(), 384);

        assert!(writer.write_previous(256, 256).is_err());
        assert_eq!(writer.byte_count(), 512);

        assert!(writer.write_previous(1, 1).is_err());
        assert_eq!(writer.byte_count(), 512);
        assert_eq!(writer.crc32(), 2733545866);

        Ok(())
    }
}
