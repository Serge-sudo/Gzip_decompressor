#![forbid(unsafe_code)]

use std::{collections::HashMap, convert::TryFrom, io::BufRead};

use anyhow::{anyhow, bail, Result};

use crate::bit_reader::{BitReader, BitSequence};
use crate::huffman_coding::LitLenToken::{EndOfBlock, Length, Literal};
use crate::huffman_coding::TreeCodeToken::{CopyPrev, RepeatZero};

////////////////////////////////////////////////////////////////////////////////

pub fn decode_litlen_distance_trees<T: BufRead>(
    bit_reader: &mut BitReader<T>,
) -> Result<(HuffmanCoding<LitLenToken>, HuffmanCoding<DistanceToken>)> {
    let mut code_lengths: [u8; 19] = [0; 19];
    let num_litlen_tokens = bit_reader.read_bits(5)?.bits() + 257;
    let num_distance_tokens = bit_reader.read_bits(5)?.bits() + 1;
    let num_code_lengths = bit_reader.read_bits(4)?.bits() + 4;

    for (num, val) in [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ]
    .iter()
    .enumerate()
    {
        if num >= num_code_lengths as usize {
            break;
        }
        code_lengths[*val as usize] = bit_reader.read_bits(3)?.bits() as u8;
    }

    let encoder = HuffmanCoding::<TreeCodeToken>::from_lengths(&code_lengths)?;

    let mut token_lengths = vec![
        Vec::<u8>::with_capacity(num_litlen_tokens as usize),
        Vec::<u8>::with_capacity(num_distance_tokens as usize),
    ];

    for length_vec in token_lengths.iter_mut() {
        while length_vec.len() < length_vec.capacity() {
            match encoder.read_symbol(bit_reader)? {
                TreeCodeToken::Length(len) => length_vec.push(len),
                CopyPrev => {
                    let copy_cnt = bit_reader.read_bits(2)?.bits() + 3;
                    let last_len = length_vec.last().copied().unwrap_or_default();
                    length_vec.resize(length_vec.len() + copy_cnt as usize, last_len);
                }
                RepeatZero { base, extra_bits } => {
                    let copy_cnt = bit_reader.read_bits(extra_bits)?.bits() + base;
                    length_vec.extend(std::iter::repeat(0).take(copy_cnt as usize));
                }
            }
        }
    }

    Ok((
        HuffmanCoding::<LitLenToken>::from_lengths(token_lengths[0].as_slice())?,
        HuffmanCoding::<DistanceToken>::from_lengths(token_lengths[1].as_slice())?,
    ))
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug)]
pub enum TreeCodeToken {
    Length(u8),
    CopyPrev,
    RepeatZero { base: u16, extra_bits: u8 },
}

impl TryFrom<HuffmanCodeWord> for TreeCodeToken {
    type Error = anyhow::Error;

    fn try_from(value: HuffmanCodeWord) -> Result<Self> {
        match value.0 {
            0..=15 => Ok(TreeCodeToken::Length(value.0 as u8)),
            16 => Ok(CopyPrev),
            17 => Ok(RepeatZero {
                base: 3,
                extra_bits: 3,
            }),
            18 => Ok(RepeatZero {
                base: 11,
                extra_bits: 7,
            }),
            _ => Err(anyhow!("Unknown value")),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug)]
pub enum LitLenToken {
    Literal(u8),
    EndOfBlock,
    Length { base: u16, extra_bits: u8 },
}

impl TryFrom<HuffmanCodeWord> for LitLenToken {
    type Error = anyhow::Error;

    fn try_from(value: HuffmanCodeWord) -> Result<Self> {
        assert!(value.0 <= 285);
        match value.0 {
            256 => Ok(EndOfBlock),
            0..=255 => Ok(Literal(value.0 as u8)),
            257..=264 => {
                let base = 3 + (value.0 - 257);
                Ok(Length {
                    base,
                    extra_bits: 0,
                })
            }
            265..=268 => {
                let shift = (value.0 - 265) * 2;
                let base = 11 + shift;
                Ok(Length {
                    base,
                    extra_bits: 1,
                })
            }
            269..=272 => {
                let shift = (value.0 - 269) * 4;
                let base = 19 + shift;
                Ok(Length {
                    base,
                    extra_bits: 2,
                })
            }
            273..=276 => {
                let shift = (value.0 - 273) * 8;
                let base = 35 + shift;
                Ok(Length {
                    base,
                    extra_bits: 3,
                })
            }
            277..=280 => {
                let shift = (value.0 - 277) * 16;
                let base = 67 + shift;
                Ok(Length {
                    base,
                    extra_bits: 4,
                })
            }
            281..=284 => {
                let shift = (value.0 - 281) * 32;
                let base = 131 + shift;
                Ok(Length {
                    base,
                    extra_bits: 5,
                })
            }
            _ => Ok(Length {
                base: 258,
                extra_bits: 0,
            }),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug)]
pub struct DistanceToken {
    pub base: u16,
    pub extra_bits: u8,
}

impl TryFrom<HuffmanCodeWord> for DistanceToken {
    type Error = anyhow::Error;

    fn try_from(value: HuffmanCodeWord) -> Result<Self> {
        const TABLE: [(u16, u8); 30] = [
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 1),
            (7, 1),
            (9, 2),
            (13, 2),
            (17, 3),
            (25, 3),
            (33, 4),
            (49, 4),
            (65, 5),
            (97, 5),
            (129, 6),
            (193, 6),
            (257, 7),
            (385, 7),
            (513, 8),
            (769, 8),
            (1025, 9),
            (1537, 9),
            (2049, 10),
            (3073, 10),
            (4097, 11),
            (6145, 11),
            (8193, 12),
            (12289, 12),
            (16385, 13),
            (24577, 13),
        ];

        if let Some(&(base, extra_bits)) = TABLE.get(value.0 as usize) {
            Ok(DistanceToken { base, extra_bits })
        } else {
            bail!("wrong code")
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

const MAX_BITS: usize = 15;

pub struct HuffmanCodeWord(pub u16);

pub struct HuffmanCoding<T> {
    map: HashMap<BitSequence, T>,
}

impl<T> HuffmanCoding<T>
where
    T: Copy + TryFrom<HuffmanCodeWord, Error = anyhow::Error>,
{
    #[allow(unused)]
    pub fn decode_symbol(&self, seq: BitSequence) -> Option<T> {
        if let Some(symbol) = self.map.get(&seq) {
            return Some(*symbol);
        }
        None
    }
    pub fn read_symbol<U: BufRead>(&self, bit_reader: &mut BitReader<U>) -> Result<T> {
        let mut result_symbol = BitSequence::new(0, 0);
        while let Ok(seq) = bit_reader.read_bits(1) {
            result_symbol = seq.concat(result_symbol);
            if let Some(val) = self.decode_symbol(result_symbol) {
                return Ok(val);
            }
        }
        bail!("couldn't read");
    }

    pub fn from_lengths(code_lengths: &[u8]) -> Result<Self> {
        let mut bl_count: HashMap<u8, u16> = HashMap::new();

        for &length in code_lengths {
            if length > 0 {
                let count = bl_count.entry(length).or_insert(0);
                *count += 1;
            }
        }

        let mut next_code = [0u16; MAX_BITS + 1];
        for bits in 1..=MAX_BITS {
            let count = bl_count.get(&(bits as u8 - 1)).unwrap_or(&0);
            next_code[bits] = (next_code[bits - 1] + count) << 1;
        }

        let mut result = HashMap::new();
        for (i, &length) in code_lengths.iter().enumerate() {
            let len = length as usize;
            if len > 0 {
                let seq = BitSequence::new(next_code[len], len as u8);
                let elem = T::try_from(HuffmanCodeWord(i as u16))?;
                result.insert(seq, elem);
                next_code[len] += 1;
            }
        }

        Ok(Self { map: result })
    }
}

////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Value(u16);

    impl TryFrom<HuffmanCodeWord> for Value {
        type Error = anyhow::Error;

        fn try_from(x: HuffmanCodeWord) -> Result<Self> {
            Ok(Self(x.0))
        }
    }

    #[test]
    fn from_lengths() -> Result<()> {
        let code = HuffmanCoding::<Value>::from_lengths(&[2, 3, 4, 3, 3, 4, 2])?;

        assert_eq!(
            code.decode_symbol(BitSequence::new(0b00, 2)),
            Some(Value(0)),
        );
        assert_eq!(
            code.decode_symbol(BitSequence::new(0b100, 3)),
            Some(Value(1)),
        );
        assert_eq!(
            code.decode_symbol(BitSequence::new(0b1110, 4)),
            Some(Value(2)),
        );
        assert_eq!(
            code.decode_symbol(BitSequence::new(0b101, 3)),
            Some(Value(3)),
        );
        assert_eq!(
            code.decode_symbol(BitSequence::new(0b110, 3)),
            Some(Value(4)),
        );
        assert_eq!(
            code.decode_symbol(BitSequence::new(0b1111, 4)),
            Some(Value(5)),
        );
        assert_eq!(
            code.decode_symbol(BitSequence::new(0b01, 2)),
            Some(Value(6)),
        );

        assert_eq!(code.decode_symbol(BitSequence::new(0b0, 1)), None);
        assert_eq!(code.decode_symbol(BitSequence::new(0b10, 2)), None);
        assert_eq!(code.decode_symbol(BitSequence::new(0b111, 3)), None,);

        Ok(())
    }

    #[test]
    fn read_symbol() -> Result<()> {
        let code = HuffmanCoding::<Value>::from_lengths(&[2, 3, 4, 3, 3, 4, 2])?;
        let mut data: &[u8] = &[0b10111001, 0b11001010, 0b11101101];
        let mut reader = BitReader::new(&mut data);

        assert_eq!(code.read_symbol(&mut reader)?, Value(1));
        assert_eq!(code.read_symbol(&mut reader)?, Value(2));
        assert_eq!(code.read_symbol(&mut reader)?, Value(3));
        assert_eq!(code.read_symbol(&mut reader)?, Value(6));
        assert_eq!(code.read_symbol(&mut reader)?, Value(0));
        assert_eq!(code.read_symbol(&mut reader)?, Value(2));
        assert_eq!(code.read_symbol(&mut reader)?, Value(4));
        assert!(code.read_symbol(&mut reader).is_err());

        Ok(())
    }

    #[test]
    fn from_lengths_with_zeros() -> Result<()> {
        let lengths = [3, 4, 5, 5, 0, 0, 6, 6, 4, 0, 6, 0, 7];
        let code = HuffmanCoding::<Value>::from_lengths(&lengths)?;
        let mut data: &[u8] = &[
            0b00100000, 0b00100001, 0b00010101, 0b10010101, 0b00110101, 0b00011101,
        ];
        let mut reader = BitReader::new(&mut data);

        assert_eq!(code.read_symbol(&mut reader)?, Value(0));
        assert_eq!(code.read_symbol(&mut reader)?, Value(1));
        assert_eq!(code.read_symbol(&mut reader)?, Value(2));
        assert_eq!(code.read_symbol(&mut reader)?, Value(3));
        assert_eq!(code.read_symbol(&mut reader)?, Value(6));
        assert_eq!(code.read_symbol(&mut reader)?, Value(7));
        assert_eq!(code.read_symbol(&mut reader)?, Value(8));
        assert_eq!(code.read_symbol(&mut reader)?, Value(10));
        assert_eq!(code.read_symbol(&mut reader)?, Value(12));
        assert!(code.read_symbol(&mut reader).is_err());

        Ok(())
    }

    #[test]
    fn from_lengths_additional() -> Result<()> {
        let lengths = [
            9, 10, 10, 8, 8, 8, 5, 6, 4, 5, 4, 5, 4, 5, 4, 4, 5, 4, 4, 5, 4, 5, 4, 5, 5, 5, 4, 6, 6,
        ];
        let code = HuffmanCoding::<Value>::from_lengths(&lengths)?;
        let mut data: &[u8] = &[
            0b11111000, 0b10111100, 0b01010001, 0b11111111, 0b00110101, 0b11111001, 0b11011111,
            0b11100001, 0b01110111, 0b10011111, 0b10111111, 0b00110100, 0b10111010, 0b11111111,
            0b11111101, 0b10010100, 0b11001110, 0b01000011, 0b11100111, 0b00000010,
        ];
        let mut reader = BitReader::new(&mut data);

        assert_eq!(code.read_symbol(&mut reader)?, Value(10));
        assert_eq!(code.read_symbol(&mut reader)?, Value(7));
        assert_eq!(code.read_symbol(&mut reader)?, Value(27));
        assert_eq!(code.read_symbol(&mut reader)?, Value(22));
        assert_eq!(code.read_symbol(&mut reader)?, Value(9));
        assert_eq!(code.read_symbol(&mut reader)?, Value(0));
        assert_eq!(code.read_symbol(&mut reader)?, Value(11));
        assert_eq!(code.read_symbol(&mut reader)?, Value(15));
        assert_eq!(code.read_symbol(&mut reader)?, Value(2));
        assert_eq!(code.read_symbol(&mut reader)?, Value(20));
        assert_eq!(code.read_symbol(&mut reader)?, Value(8));
        assert_eq!(code.read_symbol(&mut reader)?, Value(4));
        assert_eq!(code.read_symbol(&mut reader)?, Value(23));
        assert_eq!(code.read_symbol(&mut reader)?, Value(24));
        assert_eq!(code.read_symbol(&mut reader)?, Value(5));
        assert_eq!(code.read_symbol(&mut reader)?, Value(26));
        assert_eq!(code.read_symbol(&mut reader)?, Value(18));
        assert_eq!(code.read_symbol(&mut reader)?, Value(12));
        assert_eq!(code.read_symbol(&mut reader)?, Value(25));
        assert_eq!(code.read_symbol(&mut reader)?, Value(1));
        assert_eq!(code.read_symbol(&mut reader)?, Value(3));
        assert_eq!(code.read_symbol(&mut reader)?, Value(6));
        assert_eq!(code.read_symbol(&mut reader)?, Value(13));
        assert_eq!(code.read_symbol(&mut reader)?, Value(14));
        assert_eq!(code.read_symbol(&mut reader)?, Value(16));
        assert_eq!(code.read_symbol(&mut reader)?, Value(17));
        assert_eq!(code.read_symbol(&mut reader)?, Value(19));
        assert_eq!(code.read_symbol(&mut reader)?, Value(21));

        Ok(())
    }
}
