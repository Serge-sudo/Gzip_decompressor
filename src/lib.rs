#![forbid(unsafe_code)]

use crate::bit_reader::BitReader;
use crate::deflate::DeflateReader;
use crate::gzip::GzipReader;
use crate::huffman_coding::decode_litlen_distance_trees;
use crate::tracking_writer::TrackingWriter;
use anyhow::{bail, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{BufRead, Write};

mod bit_reader;
mod deflate;
mod gzip;
mod huffman_coding;
mod tracking_writer;

pub fn decompress<R: BufRead, W: Write>(input: R, mut output: W) -> Result<()> {
    let mut gzip_reader = GzipReader::new(input);
    let mut track_writer = TrackingWriter::new(&mut output);

    while let Some(header) = gzip_reader.read_header() {
        let header = header?;
        match gzip_reader.parse_header(&header) {
            Ok(mut parsed) => {
                track_writer.flush()?;
                let initial_len = track_writer.byte_count();
                let mut defl_reader = DeflateReader::new(BitReader::new(parsed.1.inner_mut()));
                process_blocks(&mut defl_reader, &mut track_writer)?;
                let footer = parsed.1.read_footer()?;
                validate_footer_data(&mut track_writer, initial_len, footer.0)?;
                gzip_reader = footer.1;
            }
            Err(error) => bail!(error),
        }
    }

    Ok(())
}

fn process_blocks<R: BufRead, W: Write>(
    defl_reader: &mut DeflateReader<R>,
    track_writer: &mut TrackingWriter<W>,
) -> Result<()> {
    loop {
        let block_res = match defl_reader.next_block() {
            Some(res) => res,
            None => break,
        };
        let (block_hdr, rdr) = match block_res {
            Ok(res) => res,
            Err(e) => return Err(e),
        };
        match block_hdr.compression_type {
            deflate::CompressionType::Uncompressed => {
                process_uncompressed_block(rdr, track_writer)?;
            }
            deflate::CompressionType::DynamicTree => {
                process_dynamic_tree_block(rdr, track_writer)?;
            }
            _ => {
                bail!("unsupported block type");
            }
        }
        if block_hdr.is_final {
            break;
        }
    }
    Ok(())
}

fn process_uncompressed_block<R: BufRead, W: Write>(
    rdr: &mut BitReader<R>,
    track_writer: &mut TrackingWriter<W>,
) -> Result<()> {
    let rdr = rdr.borrow_reader_from_boundary();
    let length = rdr.read_u16::<LittleEndian>()?;

    if length != !rdr.read_u16::<LittleEndian>()? {
        bail!("nlen check failed");
    }

    let mut buffer = vec![0; length as usize];
    rdr.read_exact(&mut buffer)?;

    track_writer.write_all(&buffer)?;
    Ok(())
}

fn process_dynamic_tree_block<R: BufRead, W: Write>(
    rdr: &mut BitReader<R>,
    track_writer: &mut TrackingWriter<W>,
) -> Result<()> {
    let (lit_length, dist) = decode_litlen_distance_trees(rdr)?;

    while let Ok(token) = lit_length.read_symbol(rdr) {
        match token {
            huffman_coding::LitLenToken::Length { base, extra_bits } => {
                let size = base + rdr.read_bits(extra_bits)?.bits();
                let token = dist.read_symbol(rdr)?;
                let distance = token.base + rdr.read_bits(token.extra_bits)?.bits();
                track_writer.write_previous(distance as usize, size as usize)?;
            }
            huffman_coding::LitLenToken::Literal(value) => {
                track_writer.write_all(&[value])?;
            }
            huffman_coding::LitLenToken::EndOfBlock => {
                break;
            }
        }
    }
    Ok(())
}

fn validate_footer_data<W: Write>(
    track_writer: &mut TrackingWriter<W>,
    initial_len: usize,
    footer_data: gzip::MemberFooter,
) -> Result<()> {
    let byte_count = track_writer.byte_count();
    let expected_len = initial_len + footer_data.data_size as usize;
    let crc32 = track_writer.crc32();

    if byte_count != expected_len {
        bail!("length check failed");
    }

    if footer_data.data_crc32 != crc32 {
        bail!("crc32 check failed");
    }

    Ok(())
}
