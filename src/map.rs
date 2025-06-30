use std::{
    fs::{File, OpenOptions},
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    num::TryFromIntError,
};

use bitfield_struct::bitfield;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use derive_more::derive::{Deref, DerefMut, From, Into};
use endian_num::le16;
use grid::Grid;
use itertools::Itertools;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rgb::Rgba;
use thiserror::Error;

use crate::{
    compress,
    consts::{
        fs_std_data_path, fs_std_overlay_path, BATTLE_MAP_WIDTH, BATTLE_TILESET_PIXEL_FORMAT,
        FIELD_MAP_CHUNK_TABLE_ADDRESS, FMAPDATA_OFFSET_TABLE_LENGTH_ADDRESS, NUMBER_OF_FIELD_MAPS,
        STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT, STANDARD_FILE_ALIGNMENT, TILE_AREA,
        TREASURE_INFO_OFFSET_TABLE_LENGTH_ADDRESS,
    },
    decompress,
    misc::{
        Bgr555, DataWithOffsetTable, DataWithOffsetTableDeserializationError,
        DataWithOffsetTableSerializationError, MaybeCompressedData, MaybeSerialized, Palette,
        PaletteDeserializationError,
    },
    utils::{
        empty_if_none, necessary_padding_for, none_if_empty, option_to_u32_or_max_try_into,
        u32_or_max_to_option_try_into, AlignToElements,
    },
    CompressionError, DecompressionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum PixelFormat {
    FourBitsPerPixel = 0,
    EightBitsPerPixel = 1,
}

impl PixelFormat {
    // For `bitfield_struct`.
    const fn from_bits(value: u8) -> Self {
        match value {
            0 => Self::FourBitsPerPixel,
            _ => Self::EightBitsPerPixel, // Hack which works if you always use `#[bits(1)]`.
        }
    }
    const fn into_bits(self) -> u8 {
        self as _
    }
    const fn array3_from_bits(value: u8) -> [Self; 3] {
        [
            Self::from_bits(value & 1),
            Self::from_bits((value >> 1) & 1),
            Self::from_bits((value >> 2) & 1),
        ]
    }
    const fn array3_into_bits(value: [Self; 3]) -> u8 {
        value[0].into_bits() | (value[1].into_bits() << 1) | (value[2].into_bits() << 2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TilesetTile(pub [u8; TILE_AREA]);

#[derive(Error, Debug)]
pub enum TilesetTileDeserializationError {
    #[error("invalid input length")]
    InvalidInputLength,
}
#[derive(Error, Debug)]
pub enum TilesetTileSerializationError {
    #[error("a pixel's value is too large to fit in {pixel_format:?}")]
    PixelValueTooLarge { pixel_format: PixelFormat },
}
#[derive(Error, Debug)]
pub enum TilesetTileFromColorsError {
    #[error("a pixel's color is not in the palette")]
    ColorNotInPalette,
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
}

impl TilesetTile {
    pub fn from_bytes(
        data: &[u8],
        pixel_format: PixelFormat,
    ) -> Result<Self, TilesetTileDeserializationError> {
        Ok(Self(match pixel_format {
            PixelFormat::FourBitsPerPixel => data
                .iter()
                .flat_map(|x| [x & 0x0F, x >> 4])
                .collect::<Vec<_>>()
                .try_into()
                .or(Err(TilesetTileDeserializationError::InvalidInputLength))?,
            PixelFormat::EightBitsPerPixel => data
                .try_into()
                .or(Err(TilesetTileDeserializationError::InvalidInputLength))?,
        }))
    }

    pub fn to_bytes(
        &self,
        pixel_format: PixelFormat,
    ) -> Result<Vec<u8>, TilesetTileSerializationError> {
        Ok(match pixel_format {
            PixelFormat::FourBitsPerPixel => self
                .0
                .chunks_exact(2)
                .map(|pixels| {
                    if pixels[0] > 0x0F || pixels[1] > 0x0F {
                        return Err(TilesetTileSerializationError::PixelValueTooLarge {
                            pixel_format,
                        });
                    }
                    Ok(pixels[0] | (pixels[1] << 4))
                })
                .collect::<Result<Vec<_>, _>>()?,
            PixelFormat::EightBitsPerPixel => self.0.to_vec(),
        })
    }

    #[inline]
    pub fn as_bgr555(&self, palette: &Palette) -> [Bgr555; TILE_AREA] {
        self.as_bgr555_with_offset(palette, 0)
    }
    #[inline]
    pub fn as_bgr555_with_offset(
        &self,
        palette: &Palette,
        palette_offset: usize,
    ) -> [Bgr555; TILE_AREA] {
        self.0.map(|x| palette.0[usize::from(x) + palette_offset])
    }
    #[inline]
    pub fn as_rgba8888(&self, palette: &Palette) -> [Rgba<u8>; TILE_AREA] {
        self.as_rgba8888_with_offset(palette, 0)
    }
    #[inline]
    pub fn as_rgba8888_with_offset(
        &self,
        palette: &Palette,
        palette_offset: usize,
    ) -> [Rgba<u8>; TILE_AREA] {
        self.0
            .map(|x| palette.color_as_rgba8888_with_offset(x.into(), palette_offset))
    }

    #[inline]
    pub fn from_bgr555_or_transparent(
        colors: &[Option<Bgr555>; TILE_AREA],
        palette: &Palette,
    ) -> Result<Self, TilesetTileFromColorsError> {
        // UNSTABLE: Use `array::try_map`.
        Ok(Self(
            colors
                .iter()
                .map(|color| -> Result<_, TilesetTileFromColorsError> {
                    Ok(if let Some(color) = color {
                        (palette
                            .0
                            .iter()
                            .skip(1)
                            .position(|x| x == color)
                            .ok_or(TilesetTileFromColorsError::ColorNotInPalette)?
                            + 1)
                        .try_into()?
                    } else {
                        0
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .unwrap(),
        ))
    }
    pub fn from_rgba8888(
        colors: &[Rgba<u8>; TILE_AREA],
        palette: &Palette,
    ) -> Result<Self, TilesetTileFromColorsError> {
        Self::from_bgr555_or_transparent(
            &colors.map(|color| {
                if color.a == 0 {
                    None
                } else {
                    Some(color.rgb().into())
                }
            }),
            palette,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Tileset(pub Vec<TilesetTile>);

impl Tileset {
    pub fn from_bytes(
        data: &[u8],
        pixel_format: PixelFormat,
    ) -> Result<Self, TilesetTileDeserializationError> {
        Ok(Self(
            data.chunks(match pixel_format {
                PixelFormat::FourBitsPerPixel => TILE_AREA / 2,
                PixelFormat::EightBitsPerPixel => TILE_AREA,
            })
            .map(|d| TilesetTile::from_bytes(d, pixel_format))
            .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    pub fn to_bytes(
        &self,
        pixel_format: PixelFormat,
    ) -> Result<Vec<u8>, TilesetTileSerializationError> {
        self.0
            .iter()
            .map(|x| x.to_bytes(pixel_format))
            .flatten_ok()
            .collect()
    }
}

#[bitfield(u16, repr = le16, from = le16::from_ne, into = le16::to_ne)]
#[derive(PartialEq, Eq, Hash)]
pub struct Tile {
    #[bits(10)]
    pub tileset_tile_id: u16,
    pub flipped_horizontally: bool,
    pub flipped_vertically: bool,
    #[bits(4)]
    pub palette_offset: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, From, Into, Deref, DerefMut)]
pub struct TileLayer(pub Grid<Tile>);

impl TileLayer {
    pub fn from_bytes(data: &[u8], width: usize) -> Self {
        Self(Grid::from_vec(
            // UNSTABLE: Use `slice::array_chunks`.
            data.chunks_exact(2)
                .map(|d| le16::from_le_bytes(d.try_into().unwrap()).into())
                .collect(),
            width,
        ))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|x| x.into_bits().to_le_bytes())
            .collect()
    }
}

#[bitfield(u8)]
#[derive(PartialEq, Eq, Hash)]
pub struct TilesetsProperties {
    #[bits(3, from = PixelFormat::array3_from_bits, into = PixelFormat::array3_into_bits)]
    pub tileset_pixel_formats: [PixelFormat; 3],
    #[bits(5)]
    pub unk: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldMapProperties {
    pub width: u16,
    pub height: u16,
    pub unk_0x04: u8,
    pub tilesets_properties: TilesetsProperties,
    pub unk_0x06: [u8; 6],
}

impl FieldMapProperties {
    pub fn from_reader(mut inp: impl Read) -> io::Result<Self> {
        Ok(Self {
            width: inp.read_u16::<LittleEndian>()?,
            height: inp.read_u16::<LittleEndian>()?,
            unk_0x04: inp.read_u8()?,
            tilesets_properties: inp.read_u8()?.into(),
            unk_0x06: {
                let mut buf = [0u8; 6];
                inp.read_exact(&mut buf)?;
                buf
            },
        })
    }

    pub fn to_writer(&self, mut out: impl Write) -> io::Result<()> {
        out.write_u16::<LittleEndian>(self.width)?;
        out.write_u16::<LittleEndian>(self.height)?;
        out.write_u8(self.unk_0x04)?;
        out.write_u8(self.tilesets_properties.into_bits())?;
        out.write_all(&self.unk_0x06)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldMapChunk {
    pub tile_layers: [Option<TileLayer>; 3],
    pub palettes: [Option<Palette>; 3],
    pub properties: FieldMapProperties,
    pub unk7: Vec<u8>,
    pub unk8: Vec<u8>,
    pub unk9: Option<DataWithOffsetTable>,
    pub unk10: Option<DataWithOffsetTable>,
    pub unk11: Vec<u8>,
    pub unk12: Vec<u8>,
    pub unk13: Vec<u8>,
    pub unk14: Vec<u8>,
    pub unk15: Vec<u8>,
    pub unk16: Vec<u8>,
    pub padding: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum FieldMapChunkFromTableError {
    #[error("the input must have exactly 17 chunks, not {0}")]
    InvalidNumberOfChunks(usize),
    #[error(transparent)]
    DataWithOffsetTableDeserialization(#[from] DataWithOffsetTableDeserializationError),
    #[error(transparent)]
    PaletteDeserialization(#[from] PaletteDeserializationError),
    #[error(transparent)]
    Io(#[from] io::Error),
}
#[derive(Error, Debug)]
pub enum FieldMapChunkIntoTableError {
    #[error(transparent)]
    DataWithOffsetTableSerialization(#[from] DataWithOffsetTableSerializationError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl TryFrom<DataWithOffsetTable> for FieldMapChunk {
    type Error = FieldMapChunkFromTableError;

    fn try_from(mut value: DataWithOffsetTable) -> Result<Self, Self::Error> {
        let chunks_len = value.chunks.len();
        if chunks_len != 17 {
            return Err(Self::Error::InvalidNumberOfChunks(chunks_len));
        }

        let properties = FieldMapProperties::from_reader(&value.chunks[6][..])?;
        Ok(Self {
            unk16: value.chunks.pop().unwrap(),
            unk15: value.chunks.pop().unwrap(),
            unk14: value.chunks.pop().unwrap(),
            unk13: value.chunks.pop().unwrap(),
            unk12: value.chunks.pop().unwrap(),
            unk11: value.chunks.pop().unwrap(),
            unk10: none_if_empty(value.chunks.pop().unwrap())
                .map(|x| DataWithOffsetTable::from_reader(&x[..]))
                .transpose()?,
            unk9: none_if_empty(value.chunks.pop().unwrap())
                .map(|x| DataWithOffsetTable::from_reader(&x[..]))
                .transpose()?,
            unk8: value.chunks.pop().unwrap(),
            unk7: value.chunks.pop().unwrap(),
            // UNSABLE: Use `array::try_map`.
            palettes: value.chunks[3..=5]
                .iter()
                .map(|x| none_if_empty(x).map(|x| Palette::from_bytes(x)).transpose())
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .unwrap(),
            tile_layers: value.chunks[0..=2]
                .iter()
                .map(|x| {
                    none_if_empty(x).map(|x| TileLayer::from_bytes(x, properties.width.into()))
                })
                .collect_array()
                .unwrap(),
            properties,
            padding: value.footer,
        })
    }
}
impl TryFrom<FieldMapChunk> for DataWithOffsetTable {
    type Error = FieldMapChunkIntoTableError;

    fn try_from(value: FieldMapChunk) -> Result<Self, Self::Error> {
        Ok(Self {
            chunks: value
                .tile_layers
                .iter()
                .map(|x| empty_if_none(x.as_ref().map(|x| x.to_bytes())))
                .chain(
                    value
                        .palettes
                        .iter()
                        .map(|x| empty_if_none(x.as_ref().map(|x| x.to_bytes()))),
                )
                .chain([
                    {
                        let mut buf = Vec::new();
                        value.properties.to_writer(&mut buf)?;
                        buf
                    },
                    value.unk7,
                    value.unk8,
                    {
                        let mut buf = Vec::new();
                        if let Some(mut value) = value.unk9 {
                            value.to_writer(&mut buf, None, true)?;
                        }
                        buf
                    },
                    {
                        let mut buf = Vec::new();
                        if let Some(mut value) = value.unk10 {
                            value.to_writer(&mut buf, None, true)?;
                        }
                        buf
                    },
                    value.unk11,
                    value.unk12,
                    value.unk13,
                    value.unk14,
                    value.unk15,
                    value.unk16,
                ])
                .collect(),
            footer: value.padding,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldMap {
    pub tileset_indexes: [Option<usize>; 3],
    pub map_chunk_index: usize,
    pub treasure_data_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldMaps {
    pub fmapdata_chunks: Vec<MaybeCompressedData>,
    pub fmapdata_padding: Vec<u8>,
    pub treasure_data: Vec<Vec<u8>>,
    pub treasure_info_padding: Vec<u8>,
    pub maps: Vec<FieldMap>,
}

#[derive(Error, Debug)]
pub enum FieldMapsFromFilesError {
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
    #[error(transparent)]
    Io(#[from] io::Error),
}
#[derive(Error, Debug)]
pub enum FieldMapsToFilesError {
    #[error("`self.maps` must contain exactly {expected} elements, not {0}", expected = NUMBER_OF_FIELD_MAPS)]
    IncorrectNumberOfMaps(usize),
    #[error(transparent)]
    Compression(#[from] CompressionError),
    #[error(transparent)]
    TryFromInt(#[from] TryFromIntError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl FieldMaps {
    pub fn from_files(
        mut fmapdata: impl Read,
        mut treasure_info: impl Read,
        mut overlay3: impl Read + Seek,
        mut overlay4: impl Read + Seek,
    ) -> Result<Self, FieldMapsFromFilesError> {
        overlay3.seek(SeekFrom::Start(FMAPDATA_OFFSET_TABLE_LENGTH_ADDRESS))?;
        let mut fmapdata_offset_table =
            vec![0; (usize::try_from(overlay3.read_u32::<LittleEndian>()?)? / 4) - 1];
        overlay3.read_u32_into::<LittleEndian>(&mut fmapdata_offset_table)?;
        overlay4.seek(SeekFrom::Start(TREASURE_INFO_OFFSET_TABLE_LENGTH_ADDRESS))?;
        let mut treasure_info_offset_table =
            vec![0; (usize::try_from(overlay4.read_u32::<LittleEndian>()?)? / 4) - 1];
        overlay4.read_u32_into::<LittleEndian>(&mut treasure_info_offset_table)?;
        overlay3.seek(SeekFrom::Start(FIELD_MAP_CHUNK_TABLE_ADDRESS))?;
        let mut chunk_table = [0; NUMBER_OF_FIELD_MAPS * 5];
        overlay3.read_u32_into::<LittleEndian>(&mut chunk_table)?;

        Ok(Self {
            fmapdata_chunks: fmapdata_offset_table
                .windows(2)
                .map(|offset_pair| -> Result<_, FieldMapsFromFilesError> {
                    let (current_offset, next_offset) = (offset_pair[0], offset_pair[1]);
                    let mut buf = vec![0u8; (next_offset - current_offset).try_into()?];
                    fmapdata.read_exact(&mut buf)?;
                    Ok(MaybeCompressedData::Compressed(buf))
                })
                .collect::<Result<Vec<_>, _>>()?,
            fmapdata_padding: {
                let mut buf: Vec<u8> = Vec::new();
                fmapdata.read_to_end(&mut buf)?;
                buf
            },
            treasure_data: treasure_info_offset_table
                .windows(2)
                .map(|offset_pair| -> Result<_, FieldMapsFromFilesError> {
                    let (current_offset, next_offset) = (offset_pair[0], offset_pair[1]);
                    let mut buf = vec![0u8; (next_offset - current_offset).try_into()?];
                    treasure_info.read_exact(&mut buf)?;
                    Ok(buf)
                })
                .collect::<Result<Vec<_>, _>>()?,
            treasure_info_padding: {
                let mut buf: Vec<u8> = Vec::new();
                treasure_info.read_to_end(&mut buf)?;
                buf
            },
            maps: chunk_table
                .chunks_exact(5)
                .map(|map| -> Result<_, FieldMapsFromFilesError> {
                    Ok(FieldMap {
                        tileset_indexes: [
                            u32_or_max_to_option_try_into(map[0])?,
                            u32_or_max_to_option_try_into(map[1])?,
                            u32_or_max_to_option_try_into(map[2])?,
                        ],
                        map_chunk_index: map[3].try_into()?,
                        treasure_data_index: u32_or_max_to_option_try_into(map[4])?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub fn to_files(
        &self,
        mut fmapdata: impl Write,
        mut treasure_info: impl Write,
        mut overlay3: impl Write + Seek,
        mut overlay4: impl Write + Seek,
        align_files: bool,
    ) -> Result<(), FieldMapsToFilesError> {
        let maps_len = self.maps.len();
        if maps_len != NUMBER_OF_FIELD_MAPS {
            return Err(FieldMapsToFilesError::IncorrectNumberOfMaps(maps_len));
        }

        overlay3.seek(SeekFrom::Start(FMAPDATA_OFFSET_TABLE_LENGTH_ADDRESS))?;
        overlay3.write_u32::<LittleEndian>((u32::try_from(self.fmapdata_chunks.len())? + 2) * 4)?;
        let mut current_fmapdata_offset = 0;
        overlay3.write_u32::<LittleEndian>(current_fmapdata_offset)?;
        for chunk in &self.fmapdata_chunks {
            let data = chunk.to_compressed()?;
            fmapdata.write_all(&data)?;
            let padding =
                necessary_padding_for(data.len(), STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT);
            fmapdata.write_all(&vec![0u8; padding])?;
            current_fmapdata_offset += u32::try_from(data.len() + padding)?;
            overlay3.write_u32::<LittleEndian>(current_fmapdata_offset)?;
        }
        if align_files {
            fmapdata.write_all(&vec![
                0u8;
                necessary_padding_for(
                    current_fmapdata_offset.try_into()?,
                    STANDARD_FILE_ALIGNMENT
                )
            ])?;
        } else {
            fmapdata.write_all(&self.fmapdata_padding)?;
        }
        overlay4.seek(SeekFrom::Start(TREASURE_INFO_OFFSET_TABLE_LENGTH_ADDRESS))?;
        overlay4.write_u32::<LittleEndian>((u32::try_from(self.treasure_data.len())? + 2) * 4)?;
        let mut current_treasure_info_offset = 0;
        overlay4.write_u32::<LittleEndian>(current_treasure_info_offset)?;
        for chunk in &self.treasure_data {
            treasure_info.write_all(chunk)?;
            let padding =
                necessary_padding_for(chunk.len(), STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT);
            fmapdata.write_all(&vec![0u8; padding])?;
            current_treasure_info_offset += u32::try_from(chunk.len() + padding)?;
            overlay4.write_u32::<LittleEndian>(current_treasure_info_offset)?;
        }
        if align_files {
            treasure_info.write_all(&vec![
                0u8;
                necessary_padding_for(
                    current_treasure_info_offset.try_into()?,
                    STANDARD_FILE_ALIGNMENT
                )
            ])?;
        } else {
            treasure_info.write_all(&self.treasure_info_padding)?;
        }

        overlay3.seek(SeekFrom::Start(FIELD_MAP_CHUNK_TABLE_ADDRESS))?;
        for map in &self.maps {
            for tileset_index in map.tileset_indexes {
                overlay3
                    .write_u32::<LittleEndian>(option_to_u32_or_max_try_into(tileset_index)?)?;
            }
            overlay3.write_u32::<LittleEndian>(map.map_chunk_index.try_into()?)?;
            overlay3.write_u32::<LittleEndian>(option_to_u32_or_max_try_into(
                map.treasure_data_index,
            )?)?;
        }

        Ok(())
    }

    pub fn load_from_fs_std() -> Result<Self, FieldMapsFromFilesError> {
        Self::from_files(
            File::open(fs_std_data_path("FMap/FMapData.dat"))?,
            File::open(fs_std_data_path("Treasure/TreasureInfo.dat"))?,
            File::open(fs_std_overlay_path(3))?,
            File::open(fs_std_overlay_path(4))?,
        )
    }
    pub fn save_to_fs_std(&self, align_files: bool) -> Result<(), FieldMapsToFilesError> {
        let mut overlay_options = OpenOptions::new();
        overlay_options.write(true);
        self.to_files(
            File::create(fs_std_data_path("FMap/FMapData.dat"))?,
            File::create(fs_std_data_path("Treasure/TreasureInfo.dat"))?,
            overlay_options.open(fs_std_overlay_path(3))?,
            overlay_options.open(fs_std_overlay_path(4))?,
            align_files,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BattleMap {
    pub unk0: Vec<u8>,
    /// Compressing and decompressing the tileset is slow,
    /// so you should only deserialize it when necessary.
    pub tileset: MaybeSerialized<Tileset>,
    pub palette: Palette,
    pub tile_layers: [TileLayer; 3],
    pub unk6: Vec<u8>,
    pub unk7: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum BattleMapTilesetDeserializationError {
    #[error(transparent)]
    Decompression(#[from] DecompressionError),
    #[error(transparent)]
    TilesetTileDeserialization(#[from] TilesetTileDeserializationError),
}
#[derive(Error, Debug)]
pub enum BattleMapTilesetSerializationError {
    #[error(transparent)]
    TilesetTileSerialization(#[from] TilesetTileSerializationError),
    #[error(transparent)]
    Compression(#[from] CompressionError),
}

impl BattleMap {
    pub fn deserialize_tileset(
        data: &[u8],
    ) -> Result<Tileset, BattleMapTilesetDeserializationError> {
        let mut buf = Cursor::new(Vec::new());
        decompress(Cursor::new(data), &mut buf, false)?;
        let mut buf = buf.into_inner();
        buf.align_to_elements(TILE_AREA / 2);
        Ok(Tileset::from_bytes(&buf, BATTLE_TILESET_PIXEL_FORMAT)?)
    }
    pub fn serialize_tileset(
        tileset: &Tileset,
    ) -> Result<Vec<u8>, BattleMapTilesetSerializationError> {
        let uncompressed = tileset.to_bytes(BATTLE_TILESET_PIXEL_FORMAT)?;
        let last_non_zero = uncompressed
            .iter()
            .rposition(|&x| x != 0)
            .unwrap_or(uncompressed.len());
        let mut buf = Cursor::new(Vec::new());
        compress(&uncompressed[..=last_non_zero], &mut buf)?;
        Ok(buf.into_inner())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BattleMapFile {
    pub maps: Vec<BattleMap>,
    pub unk_last: [Vec<u8>; 9],
    pub padding: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum BattleMapFileFromTableError {
    #[error("the number of chunks of the input ({0}) minus 1 isn't divisible by 8")]
    InvalidNumberOfChunks(usize),
    #[error(transparent)]
    PaletteDeserialization(#[from] PaletteDeserializationError),
}
#[derive(Error, Debug)]
pub enum BattleMapFileIntoTableError {
    #[error(transparent)]
    BattleMapTilesetSerialization(#[from] BattleMapTilesetSerializationError),
}

impl TryFrom<DataWithOffsetTable> for BattleMapFile {
    type Error = BattleMapFileFromTableError;

    fn try_from(mut value: DataWithOffsetTable) -> Result<Self, Self::Error> {
        let chunks_len = value.chunks.len();
        if chunks_len % 8 != 1 {
            return Err(Self::Error::InvalidNumberOfChunks(chunks_len));
        }

        Ok(Self {
            unk_last: value.chunks.split_off(chunks_len - 9).try_into().unwrap(),
            maps: value
                .chunks
                .into_iter()
                // UNSTABLE: Use `Iterator::array_chunks`.
                .chunks(8)
                .into_iter()
                .map(|mut chunks| -> Result<_, Self::Error> {
                    Ok(BattleMap {
                        unk0: chunks.next().unwrap(),
                        tileset: MaybeSerialized::Serialized(chunks.next().unwrap()),
                        palette: Palette::from_bytes(&chunks.next().unwrap())?,
                        tile_layers: chunks
                            .by_ref()
                            .take(3)
                            .map(|x| TileLayer::from_bytes(&x, BATTLE_MAP_WIDTH))
                            .collect::<Vec<_>>()
                            .try_into()
                            .unwrap(),
                        unk6: chunks.next().unwrap(),
                        unk7: chunks.next().unwrap(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            padding: value.footer,
        })
    }
}
impl TryFrom<BattleMapFile> for DataWithOffsetTable {
    type Error = BattleMapFileIntoTableError;

    fn try_from(value: BattleMapFile) -> Result<Self, Self::Error> {
        Ok(Self {
            chunks: value
                .maps
                .into_iter()
                .map(|map| -> Result<_, Self::Error> {
                    Ok([
                        map.unk0,
                        match map.tileset {
                            MaybeSerialized::Serialized(data) => data,
                            MaybeSerialized::Deserialized(tileset) => {
                                BattleMap::serialize_tileset(&tileset)?
                            }
                        },
                        map.palette.to_bytes(),
                    ]
                    .into_iter()
                    .chain(map.tile_layers.into_iter().map(|x| x.to_bytes()))
                    .chain([map.unk6, map.unk7]))
                })
                .flatten_ok()
                .chain(value.unk_last.into_iter().map(Ok))
                .collect::<Result<Vec<_>, _>>()?,
            footer: value.padding,
        })
    }
}
