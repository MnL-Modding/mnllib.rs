use std::{
    borrow::Cow,
    fmt::Display,
    io::{self, Cursor, Read, Write},
    num::TryFromIntError,
};

use bitfield_struct::bitfield;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use endian_num::le16;
use rgb::{Rgb, Rgba};
use thiserror::Error;

use crate::{compress, decompress, utils::AlignToElements, CompressionError, DecompressionError};

pub fn filesystem_standard_data_path(filename: impl Display) -> String {
    format!("data/data/{}", filename)
}
pub fn filesystem_standard_overlay_path(overlay_number: impl Display) -> String {
    format!("data/overlay.dec/overlay_{:04}.dec.bin", overlay_number)
}

pub trait VarIntReader {
    fn read_varint(&mut self) -> io::Result<u32>;
}

impl<T: Read> VarIntReader for T {
    fn read_varint(&mut self) -> io::Result<u32> {
        let data = self.read_u8()?;
        let size = data >> 6;
        let mut result = u32::from(data & 0b00111111);
        for i in 0..size {
            result |= u32::from(self.read_u8()?) << ((i + 1) * 6);
        }
        Ok(result)
    }
}

pub trait VarInt {
    fn encode_var(self) -> Vec<u8>;
}

impl VarInt for u32 {
    fn encode_var(mut self) -> Vec<u8> {
        let mut result = vec![(self & 0b00111111) as u8];
        self >>= 6;
        while self > 255 {
            result.push(self as u8);
            result[0] += 1 << 6;
            self >>= 6;
        }
        if self > 0 {
            result.push(self as u8);
            result[0] += 1 << 6;
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaybeCompressedData {
    Uncompressed(Vec<u8>),
    Compressed(Vec<u8>),
}

impl MaybeCompressedData {
    pub fn to_uncompressed(&self, strict: bool) -> Result<Cow<[u8]>, DecompressionError> {
        Ok(match self {
            Self::Uncompressed(data) => Cow::Borrowed(data),
            Self::Compressed(data) => {
                let mut buf = Cursor::new(Vec::new());
                decompress(Cursor::new(data), &mut buf, strict)?;
                Cow::Owned(buf.into_inner())
            }
        })
    }
    /// Decompresses the data in-place if it isn't uncompressed already,
    /// and returns a mutable reference to the uncompressed data inside `self`.
    pub fn make_uncompressed(&mut self, strict: bool) -> Result<&mut Vec<u8>, DecompressionError> {
        Ok(match self {
            Self::Uncompressed(data) => data,
            Self::Compressed(data) => {
                let mut buf = Cursor::new(Vec::new());
                decompress(Cursor::new(data), &mut buf, strict)?;
                *self = Self::Uncompressed(buf.into_inner());
                match self {
                    Self::Uncompressed(data) => data,
                    _ => unreachable!(),
                }
            }
        })
    }

    pub fn to_compressed(&self) -> Result<Cow<[u8]>, CompressionError> {
        Ok(match self {
            Self::Compressed(data) => Cow::Borrowed(data),
            Self::Uncompressed(data) => {
                let mut buf = Cursor::new(Vec::new());
                compress(data, &mut buf)?;
                Cow::Owned(buf.into_inner())
            }
        })
    }
    /// Compresses the data in-place if it isn't compressed already,
    /// and returns a mutable reference to the compressed data inside `self`.
    pub fn make_compressed(&mut self) -> Result<&mut Vec<u8>, CompressionError> {
        Ok(match self {
            Self::Compressed(data) => data,
            Self::Uncompressed(data) => {
                let mut buf = Cursor::new(Vec::new());
                compress(data, &mut buf)?;
                *self = Self::Compressed(buf.into_inner());
                match self {
                    Self::Compressed(data) => data,
                    _ => unreachable!(),
                }
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaybeSerialized<T> {
    Serialized(Vec<u8>),
    Deserialized(T),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataWithOffsetTable {
    pub chunks: Vec<Vec<u8>>,
    pub footer: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum DataWithOffsetTableDeserializationError {
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
    #[error(transparent)]
    Io(#[from] io::Error),
}
#[derive(Error, Debug)]
pub enum DataWithOffsetTableSerializationError {
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl DataWithOffsetTable {
    pub fn from_reader(
        mut inp: impl Read,
    ) -> Result<Self, DataWithOffsetTableDeserializationError> {
        let first_offset = inp.read_u32::<LittleEndian>()?;
        let (num_offsets, padding) = (first_offset / 4, first_offset % 4);
        let mut offsets: Vec<u32> = Vec::with_capacity(num_offsets.try_into()?);
        offsets.push(first_offset);
        for _ in 1..num_offsets {
            offsets.push(inp.read_u32::<LittleEndian>()?);
        }
        if padding != 0 {
            // Alternative to seeking so that we don't require `Seek` for this one operation.
            let mut padding_buf = vec![0u8; padding.try_into()?];
            inp.read_exact(&mut padding_buf)?;
        }

        Ok(Self {
            chunks: offsets
                // UNSTABLE: Use `slice::array_windows`.
                .windows(2)
                .map(
                    |offset_pair| -> Result<_, DataWithOffsetTableDeserializationError> {
                        let (current_offset, next_offset) = (offset_pair[0], offset_pair[1]);
                        let mut buf = vec![0u8; (next_offset - current_offset).try_into()?];
                        inp.read_exact(&mut buf)?;
                        Ok(buf)
                    },
                )
                .collect::<Result<Vec<_>, _>>()?,
            footer: {
                let mut buf: Vec<u8> = Vec::new();
                inp.read_to_end(&mut buf)?;
                buf
            },
        })
    }

    /// If `chunk_alignment` is set, this function will align
    /// `self.chunks` in-place, mutating them.
    pub fn to_writer(
        &mut self,
        mut out: impl Write,
        chunk_alignment: Option<usize>,
        write_footer: bool,
    ) -> Result<(), DataWithOffsetTableSerializationError> {
        let mut current_offset = (self.chunks.len() + 1) * 4;
        out.write_u32::<LittleEndian>(current_offset.try_into()?)?;
        for chunk in &mut self.chunks {
            if let Some(alignment) = chunk_alignment {
                chunk.align_to_elements(alignment);
            }
            current_offset += chunk.len();
            out.write_u32::<LittleEndian>(current_offset.try_into()?)?;
        }

        for chunk in &self.chunks {
            out.write_all(chunk)?;
        }
        if write_footer {
            out.write_all(&self.footer)?;
        }

        Ok(())
    }
}

#[bitfield(u16, new = false, repr = le16, from = le16::from_ne, into = le16::to_ne)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rgb555 {
    #[bits(5)]
    pub r: u8,
    #[bits(5)]
    pub g: u8,
    #[bits(5)]
    pub b: u8,
    __: bool, // Padding
}

impl Rgb555 {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self::default().with_r(r).with_g(g).with_b(b)
    }
    #[allow(clippy::result_unit_err)]
    pub fn new_checked(r: u8, g: u8, b: u8) -> Result<Self, ()> {
        Self::default()
            .with_r_checked(r)?
            .with_g_checked(g)?
            .with_b_checked(b)
    }
}
impl From<Rgb<u8>> for Rgb555 {
    #[inline]
    fn from(value: Rgb<u8>) -> Self {
        Self::new(value.r >> 3, value.g >> 3, value.b >> 3)
    }
}
impl From<Rgb555> for Rgb<u8> {
    #[inline]
    fn from(value: Rgb555) -> Self {
        Self::new(value.r() << 3, value.g() << 3, value.b() << 3)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Palette(pub Vec<Rgb555>);

#[derive(Error, Debug)]
pub enum PaletteDeserializationError {
    #[error("the input contains extra bytes")]
    ExtraBytesInInput,
}

impl Palette {
    pub fn from_bytes(data: &[u8]) -> Result<Self, PaletteDeserializationError> {
        if data.len() % 2 != 0 {
            return Err(PaletteDeserializationError::ExtraBytesInInput);
        }
        Ok(Self(
            // UNSTABLE: Use `slice::array_chunks`.
            data.chunks_exact(2)
                .map(|x| le16::from_le_bytes(x.try_into().unwrap()).into())
                .collect(),
        ))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|x| x.into_bits().to_le_bytes())
            .collect()
    }

    #[inline]
    pub fn color_as_rgba8888(&self, index: usize) -> Rgba<u8> {
        <Rgb<u8>>::from(self.0[index]).with_alpha(if index == 0 { 0x00 } else { 0xFF })
    }
}
