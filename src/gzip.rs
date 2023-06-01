#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, Result};
use crc::Crc;
use std::io::BufRead;
////////////////////////////////////////////////////////////////////////////////

const ID1: u8 = 0x1f;
const ID2: u8 = 0x8b;

const CM_DEFLATE: u8 = 8;

const FTEXT_OFFSET: u8 = 0;
const FHCRC_OFFSET: u8 = 1;
const FEXTRA_OFFSET: u8 = 2;
const FNAME_OFFSET: u8 = 3;
const FCOMMENT_OFFSET: u8 = 4;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct MemberHeader {
    pub compression_method: CompressionMethod,
    pub modification_time: u32,
    pub extra: Option<Vec<u8>>,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub extra_flags: u8,
    pub os: u8,
    pub has_crc: bool,
    pub is_text: bool,
}

impl MemberHeader {
    pub fn crc16(&self) -> u16 {
        let crc = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        let mut digest = crc.digest();

        digest.update(&[ID1, ID2, self.compression_method.into(), self.flags().0]);
        digest.update(&self.modification_time.to_le_bytes());
        digest.update(&[self.extra_flags, self.os]);

        if let Some(extra) = &self.extra {
            digest.update(&(extra.len() as u16).to_le_bytes());
            digest.update(extra);
        }

        if let Some(name) = &self.name {
            digest.update(name.as_bytes());
            digest.update(&[0]);
        }

        if let Some(comment) = &self.comment {
            digest.update(comment.as_bytes());
            digest.update(&[0]);
        }

        (digest.finalize() & 0xffff) as u16
    }

    pub fn flags(&self) -> MemberFlags {
        let mut flags = MemberFlags(0);
        flags.set_is_text(self.is_text);
        flags.set_has_crc(self.has_crc);
        flags.set_has_extra(self.extra.is_some());
        flags.set_has_name(self.name.is_some());
        flags.set_has_comment(self.comment.is_some());
        flags
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug)]
pub enum CompressionMethod {
    Deflate,
    Unknown(u8),
}

impl From<u8> for CompressionMethod {
    fn from(value: u8) -> Self {
        match value {
            CM_DEFLATE => Self::Deflate,
            x => Self::Unknown(x),
        }
    }
}

impl From<CompressionMethod> for u8 {
    fn from(method: CompressionMethod) -> u8 {
        match method {
            CompressionMethod::Deflate => CM_DEFLATE,
            CompressionMethod::Unknown(x) => x,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct MemberFlags(u8);

#[allow(unused)]
impl MemberFlags {
    fn bit(&self, n: u8) -> bool {
        (self.0 >> n) & 1 != 0
    }

    fn set_bit(&mut self, n: u8, value: bool) {
        if value {
            self.0 |= 1 << n;
        } else {
            self.0 &= !(1 << n);
        }
    }

    pub fn is_text(&self) -> bool {
        self.bit(FTEXT_OFFSET)
    }

    pub fn set_is_text(&mut self, value: bool) {
        self.set_bit(FTEXT_OFFSET, value)
    }

    pub fn has_crc(&self) -> bool {
        self.bit(FHCRC_OFFSET)
    }

    pub fn set_has_crc(&mut self, value: bool) {
        self.set_bit(FHCRC_OFFSET, value)
    }

    pub fn has_extra(&self) -> bool {
        self.bit(FEXTRA_OFFSET)
    }

    pub fn set_has_extra(&mut self, value: bool) {
        self.set_bit(FEXTRA_OFFSET, value)
    }

    pub fn has_name(&self) -> bool {
        self.bit(FNAME_OFFSET)
    }

    pub fn set_has_name(&mut self, value: bool) {
        self.set_bit(FNAME_OFFSET, value)
    }

    pub fn has_comment(&self) -> bool {
        self.bit(FCOMMENT_OFFSET)
    }

    pub fn set_has_comment(&mut self, value: bool) {
        self.set_bit(FCOMMENT_OFFSET, value)
    }
}

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct MemberFooter {
    pub data_crc32: u32,
    pub data_size: u32,
}

////////////////////////////////////////////////////////////////////////////////

pub struct GzipReader<T> {
    reader: T,
}

impl<T: BufRead> GzipReader<T> {
    pub fn new(reader: T) -> Self {
        Self { reader }
    }

    pub fn read_header(&mut self) -> Option<Result<[u8; 10]>> {
        let mut header = [0_u8; 10];
        match self.reader.read(&mut header) {
            Ok(size) if size == 0 => None,
            Ok(size) if size < 10 => Some(Err(anyhow!("eof error"))),
            Ok(_) => Some(Ok(header)),
            Err(err) => Some(Err(anyhow!(err))),
        }
    }

    fn read_crc16(&mut self) -> u16 {
        let mut crc_ = [0_u8; 2];
        self.reader.read_exact(&mut crc_).unwrap();
        u16::from_le_bytes(crc_)
    }

    fn read_string_until_null(&mut self) -> Option<String> {
        let mut data = Vec::new();
        self.reader.read_until(b'\0', &mut data).unwrap();
        String::from_utf8(data).ok()
    }

    fn read_extra(&mut self) -> Option<Vec<u8>> {
        let mut extra_data = Vec::new();
        let mut buffer = [0_u8; 4096];

        let mut sz_additional_lines = [0_u8; 2];
        self.reader.read_exact(&mut sz_additional_lines).ok()?;
        let len_add = u16::from_le_bytes(sz_additional_lines);

        let mut mutremaining = len_add as usize;
        while mutremaining > 0 {
            let to_read = std::cmp::min(mutremaining, buffer.len());
            let read = self.reader.read(&mut buffer[..to_read]).ok()?;
            if read == 0 {
                return None;
            }
            extra_data.extend_from_slice(&buffer[..read]);
            mutremaining -= read;
        }

        Some(extra_data)
    }

    pub fn parse_header(mut self, header_bytes: &[u8]) -> Result<(MemberHeader, MemberReader<T>)> {
        if header_bytes.first() != Some(&ID1) || header_bytes.get(1) != Some(&ID2) {
            bail!("wrong id values");
        }
        let compression_method =
            match CompressionMethod::from(header_bytes.get(2).copied().unwrap_or_default()) {
                CompressionMethod::Unknown(_) => bail!("unsupported compression method"),
                method => method,
            };
        let flags = MemberFlags(header_bytes[3]);

        let res = MemberHeader {
            compression_method,
            modification_time: u32::from_le_bytes((&header_bytes[4..8]).try_into().unwrap()),
            extra: flags.has_extra().then(|| self.read_extra()).flatten(),
            name: flags
                .has_name()
                .then(|| self.read_string_until_null())
                .flatten(),
            comment: flags
                .has_comment()
                .then(|| self.read_string_until_null())
                .flatten(),
            extra_flags: header_bytes[8],
            os: header_bytes[9],
            has_crc: flags.has_crc(),
            is_text: flags.is_text(),
        };

        let crc16 = flags
            .has_crc()
            .then(|| self.read_crc16())
            .unwrap_or_default();

        if flags.has_crc() && crc16 != res.crc16() {
            bail!("header crc16 check failed");
        }
        Ok((res, MemberReader { inner: self.reader }))
    }
}

////////////////////////////////////////////////////////////////////////////////

pub struct MemberReader<T> {
    inner: T,
}

impl<T: BufRead> MemberReader<T> {
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn read_footer(mut self) -> Result<(MemberFooter, GzipReader<T>)> {
        let mut buf = [0_u8; 8];
        self.inner.read_exact(&mut buf)?;
        let data_crc32 = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        let data_size = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let footer = MemberFooter {
            data_crc32,
            data_size,
        };
        let reader = GzipReader::new(self.inner);
        Ok((footer, reader))
    }
}
