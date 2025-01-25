use crate::map::PixelSize;

pub const STANDARD_FILE_ALIGNMENT: usize = 512;
pub const STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT: usize = 4;

pub const TILE_WIDTH: usize = 8;
pub const TILE_HEIGHT: usize = 8;
pub const TILE_AREA: usize = TILE_WIDTH * TILE_HEIGHT;

pub const BATTLE_TILESET_PIXEL_SIZE: PixelSize = PixelSize::Nibble;
pub const BATTLE_MAP_WIDTH: usize = 64;
pub const BATTLE_MAP_HEIGHT: usize = 32;

pub const NUMBER_OF_FIELD_MAPS: usize = 0x02A9;
/// Overlay 3.
pub const FMAPDATA_OFFSET_TABLE_LENGTH_ADDRESS: u64 = 0x11310;
/// Overlay 3.
pub const FMAPDATA_OFFSET_TABLE_ADDRESS: u64 = FMAPDATA_OFFSET_TABLE_LENGTH_ADDRESS + 4;
/// Overlay 4.
pub const TREASURE_INFO_OFFSET_TABLE_LENGTH_ADDRESS: u64 = 0x4AA30;
/// Overlay 4.
pub const TREASURE_INFO_OFFSET_TABLE_ADDRESS: u64 = TREASURE_INFO_OFFSET_TABLE_LENGTH_ADDRESS + 4;
/// Overlay 3.
pub const FIELD_MAP_CHUNK_TABLE_ADDRESS: u64 = 0x19FD0;
