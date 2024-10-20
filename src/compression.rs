use std::{
    cmp::{max, min},
    io::{self, Read, Seek, SeekFrom, Write},
    num::TryFromIntError,
};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use thiserror::Error;

use crate::misc::{VarInt, VarIntReader};

#[derive(Debug, Clone, Copy, Eq, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum CompressionCommand {
    EndBlock = 0,
    Copy = 1,
    Lz77 = 2,
    Rle = 3,
}

#[derive(Error, Debug)]
pub enum DecompressionError {
    #[error("invalid compression command {0}")]
    InvalidCompressionCommand(u8),
    #[error("the declared uncompressed size ({declared}) doesn't match the actual one ({actual})")]
    IncorrectUncompressedSize { declared: u32, actual: u64 },
    #[error("the declared block size ({declared}) doesn't match the actual one ({actual})")]
    IncorrectBlockSize { declared: u16, actual: u64 },
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Error, Debug)]
pub enum CompressionError {
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn decompress<R, W>(mut src: R, mut dst: W, strict: bool) -> Result<(), DecompressionError>
where
    R: Read + Seek,
    W: Read + Write + Seek,
{
    let uncompressed_size = src.read_varint()?;
    let num_blocks = src.read_varint()? + 1;

    for _ in 0..num_blocks {
        let block_size = src.read_u16::<LittleEndian>()?;
        let block_start = src.stream_position()?;

        'block: for _ in 0..256 {
            let mut commands_byte = src.read_u8()?;
            for _ in 0..4 {
                match CompressionCommand::try_from(commands_byte & 0x03)
                    .map_err(|err| DecompressionError::InvalidCompressionCommand(err.number))?
                {
                    CompressionCommand::EndBlock => break 'block,
                    CompressionCommand::Copy => {
                        let mut buf = [0u8];
                        src.read_exact(&mut buf)?;
                        dst.write_all(&buf)?;
                    }
                    CompressionCommand::Lz77 => {
                        let mut buf = [0u8; 2];
                        src.read_exact(&mut buf)?;
                        dst.seek_relative(-(i64::from(buf[0]) | (i64::from(buf[1] & 0xF0) << 4)))?;
                        let mut data_to_copy = vec![0u8; usize::from(buf[1] & 0x0F) + 2];
                        dst.read_exact(&mut data_to_copy)?;
                        dst.seek(SeekFrom::End(0))?;
                        dst.write_all(&data_to_copy)?;
                    }
                    CompressionCommand::Rle => {
                        let count = src.read_u8()? + 2;
                        let data = src.read_u8()?;
                        dst.write_all(&vec![data; count.into()])?;
                    }
                }
                commands_byte >>= 2;
            }
        }

        if strict {
            let actual_block_size = src.stream_position()? - block_start;
            if actual_block_size != block_size.into() {
                return Err(DecompressionError::IncorrectBlockSize {
                    declared: block_size,
                    actual: actual_block_size,
                });
            }
        }
    }

    if strict {
        let actual_uncompressed_size = dst.stream_position()?;
        if actual_uncompressed_size != uncompressed_size.into() {
            return Err(DecompressionError::IncorrectUncompressedSize {
                declared: uncompressed_size,
                actual: actual_uncompressed_size,
            });
        }
    }
    Ok(())
}

pub fn compress<W>(src: &[u8], mut dst: W) -> Result<(), CompressionError>
where
    W: Write + Seek,
{
    let uncompressed_size = src.len();
    dst.write_all(&u32::try_from(uncompressed_size)?.encode_var())?;
    let num_blocks = (uncompressed_size as f64 / 512.0).ceil() as u32;
    dst.write_all(&(num_blocks - 1).encode_var())?;

    for block_number in 0..num_blocks {
        let uncompressed_block_position = usize::try_from(block_number)? * 512;
        let uncompressed_block_size = min(uncompressed_size - uncompressed_block_position, 512);
        let mut uncompressed_block_offset = 0usize;
        let compressed_block_position = dst.stream_position()?;
        dst.write_u16::<LittleEndian>(0x0000)?;
        let mut last_command_number = -1i8;

        while uncompressed_block_offset < uncompressed_block_size {
            let commands_byte_position = dst.stream_position()?;
            let mut commands_byte = 0u8;
            dst.write_all(&[commands_byte])?;
            for command_number in 0..4 {
                if uncompressed_block_offset >= uncompressed_block_size {
                    break;
                }
                let current_uncompressed_position =
                    uncompressed_block_position + uncompressed_block_offset;
                let first_byte = src[current_uncompressed_position];

                let mut lz77_best_length = 0u8;
                let mut lz77_best_offset = 0u16;
                for offset in (2..=min(current_uncompressed_position, 0xFFF) as u16).rev() {
                    let mut current_length = 0u8;
                    while current_length < 17
                        && u16::from(current_length) < offset
                        && uncompressed_block_offset + usize::from(current_length)
                            < uncompressed_block_size
                    {
                        if src[current_uncompressed_position + usize::from(current_length)]
                            != src[current_uncompressed_position - usize::from(offset)
                                + usize::from(current_length)]
                        {
                            break;
                        }
                        current_length += 1;
                    }
                    if current_length > lz77_best_length {
                        lz77_best_length = current_length;
                        lz77_best_offset = offset;
                    }
                }

                let mut rle_count = 1u16;
                while uncompressed_block_offset + usize::from(rle_count) < uncompressed_block_size
                    && rle_count < 257
                {
                    if src[current_uncompressed_position + usize::from(rle_count)] != first_byte {
                        break;
                    }
                    rle_count += 1;
                }

                let current_command: CompressionCommand;
                let best_length = max(lz77_best_length.into(), rle_count);
                if best_length <= 1 {
                    current_command = CompressionCommand::Copy;
                    dst.write_all(&[first_byte])?;
                } else if u16::from(lz77_best_length) > rle_count {
                    current_command = CompressionCommand::Lz77;
                    dst.write_all(&[
                        lz77_best_offset as u8,
                        (lz77_best_length - 2) | (((lz77_best_offset & 0xF00) >> 4) as u8),
                    ])?;
                } else {
                    current_command = CompressionCommand::Rle;
                    dst.write_all(&[(rle_count - 2) as u8, first_byte])?;
                }

                commands_byte |= u8::from(current_command) << (command_number * 2);
                uncompressed_block_offset += usize::from(best_length);
                last_command_number = command_number;
            }
            dst.seek(SeekFrom::Start(commands_byte_position))?;
            dst.write_all(&[commands_byte])?;
            dst.seek(SeekFrom::End(0))?;
        }

        if last_command_number == 3 {
            dst.write_all(&[0u8])?;
        }
        let compressed_block_end_position = dst.stream_position()?;
        dst.seek(SeekFrom::Start(compressed_block_position))?;
        dst.write_u16::<LittleEndian>(
            (compressed_block_end_position - compressed_block_position - 2).try_into()?,
        )?;
        dst.seek(SeekFrom::End(0))?;
    }

    Ok(())
}
