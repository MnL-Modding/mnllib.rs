use std::io::{self, Read};

use byteorder::ReadBytesExt;

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
