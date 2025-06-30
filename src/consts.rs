use std::fmt::Display;

use crate::map::PixelFormat;

pub const DATA_DIR: &str = "data";
pub const OVERLAYS_DIR: &str = "overlay";
pub const DECOMPRESSED_OVERLAYS_DIR: &str = "overlay.dec";

pub fn fs_std_data_path(path: impl Display) -> String {
    format!("data/{DATA_DIR}/{path}")
}
pub fn fs_std_overlay_path(overlay_id: impl Display) -> String {
    format!("data/{DECOMPRESSED_OVERLAYS_DIR}/overlay_{overlay_id:04}.dec.bin")
}

pub const STANDARD_FILE_ALIGNMENT: usize = 512;
pub const STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT: usize = 4;

pub const TILE_WIDTH: usize = 8;
pub const TILE_HEIGHT: usize = 8;
pub const TILE_AREA: usize = TILE_WIDTH * TILE_HEIGHT;

pub const BATTLE_TILESET_PIXEL_FORMAT: PixelFormat = PixelFormat::FourBitsPerPixel;
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
