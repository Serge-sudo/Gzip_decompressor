#![forbid(unsafe_code)]

use std::io::{self, BufRead};

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitSequence {
    bits: u16,
    len: u8,
}

impl BitSequence {
    pub fn new(bits: u16, len: u8) -> Self {
        let new_data = match len {
            0 => bits,
            1..=15 => bits & ((1 << len) - 1),
            16 => bits,
            17.. => std::unreachable!(),
        };
        Self {
            bits: new_data,
            len,
        }
    }

    pub fn bits(&self) -> u16 {
        self.bits
    }

    pub fn len(&self) -> u8 {
        self.len
    }

    pub fn concat(self, other: Self) -> Self {
        assert!(self.len + other.len <= 16, "Too big");
        let new_bits = self.bits | other.bits << self.len;
        BitSequence::new(new_bits, self.len + other.len)
    }
}

////////////////////////////////////////////////////////////////////////////////

pub struct BitReader<T> {
    stream: T,
    bit_seq: BitSequence,
}

impl<T: BufRead> BitReader<T> {
    pub fn new(stream: T) -> Self {
        Self {
            stream,
            bit_seq: BitSequence::new(0, 0),
        }
    }

    pub fn read_bits(&mut self, len: u8) -> io::Result<BitSequence> {
        assert!(len <= 16, "len is bigger than 16");

        if self.bit_seq.len() >= len {
            let old = BitSequence::new(self.bit_seq.bits & ((1 << len) - 1), len);
            self.bit_seq.bits >>= len;
            self.bit_seq.len -= len;
            return Ok(old);
        }

        let vital_len = len - self.bit_seq.len();
        let mut temp_bytes: [u8; 2] = [0, 0];
        let temp_size = if vital_len > 8 { 2 } else { 1 };

        self.stream.read_exact(&mut temp_bytes[..temp_size])?;

        let byte = u16::from_le_bytes(temp_bytes);
        let rest = BitSequence::new(byte, vital_len);
        let new_len = 8 * temp_size as u8 - vital_len;
        let mut new_buf = BitSequence::new(byte >> vital_len, new_len);

        std::mem::swap(&mut new_buf, &mut self.bit_seq);

        Ok(new_buf.concat(rest))
    }

    /// Discard all the unread bits in the current byte and return a mutable reference
    /// to the underlying reader.
    pub fn borrow_reader_from_boundary(&mut self) -> &mut T {
        self.bit_seq = BitSequence::new(0u16, 0u8);
        &mut self.stream
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::ReadBytesExt;

    #[test]
    fn read_bits() -> io::Result<()> {
        let data: &[u8] = &[0b01100011, 0b11011011, 0b10101111];
        let mut reader = BitReader::new(data);
        assert_eq!(reader.read_bits(1)?, BitSequence::new(0b1, 1));
        assert_eq!(reader.read_bits(2)?, BitSequence::new(0b01, 2));
        assert_eq!(reader.read_bits(3)?, BitSequence::new(0b100, 3));
        assert_eq!(reader.read_bits(4)?, BitSequence::new(0b1101, 4));
        assert_eq!(reader.read_bits(5)?, BitSequence::new(0b10110, 5));
        assert_eq!(reader.read_bits(8)?, BitSequence::new(0b01011111, 8));
        assert_eq!(
            reader.read_bits(2).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        Ok(())
    }

    #[test]
    fn borrow_reader_from_boundary() -> io::Result<()> {
        let data: &[u8] = &[0b01100011, 0b11011011, 0b10101111];
        let mut reader = BitReader::new(data);
        assert_eq!(reader.read_bits(3)?, BitSequence::new(0b011, 3));
        assert_eq!(reader.borrow_reader_from_boundary().read_u8()?, 0b11011011);
        assert_eq!(reader.read_bits(8)?, BitSequence::new(0b10101111, 8));
        Ok(())
    }
}
