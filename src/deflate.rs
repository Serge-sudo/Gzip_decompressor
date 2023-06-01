#![forbid(unsafe_code)]

use std::io::BufRead;

use anyhow::Result;

use crate::bit_reader::BitReader;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct BlockHeader {
    pub is_final: bool,
    pub compression_type: CompressionType,
}

#[derive(Debug)]
pub enum CompressionType {
    Uncompressed = 0,
    FixedTree = 1,
    DynamicTree = 2,
    Reserved = 3,
}

////////////////////////////////////////////////////////////////////////////////

pub struct DeflateReader<T> {
    bit_reader: BitReader<T>,
}

impl<T: BufRead> DeflateReader<T> {
    pub fn new(bit_reader: BitReader<T>) -> Self {
        Self { bit_reader }
    }

    pub fn next_block(&mut self) -> Option<Result<(BlockHeader, &mut BitReader<T>)>> {
        let is_final = self.bit_reader.read_bits(1).ok()?.bits() == 1;
        let compression_type = match self.bit_reader.read_bits(2).ok()?.bits() {
            0 => CompressionType::Uncompressed,
            1 => CompressionType::FixedTree,
            2 => CompressionType::DynamicTree,
            _ => CompressionType::Reserved,
        };
        Some(Ok((
            BlockHeader {
                is_final,
                compression_type,
            },
            &mut self.bit_reader,
        )))
    }
}
