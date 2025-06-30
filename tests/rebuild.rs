use std::{
    fmt::{Debug, Display},
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use mnllib::{
    consts::{fs_std_data_path, fs_std_overlay_path, STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT},
    map::{BattleMap, BattleMapFile, FieldMapChunk, FieldMaps, Tileset},
    misc::{DataWithOffsetTable, MaybeCompressedData, MaybeSerialized},
};
use rstest::rstest;

fn test_path(path: impl AsRef<Path>) -> PathBuf {
    Path::new("tests").join(path)
}
fn test_fs_data_path(filename: impl Display) -> PathBuf {
    test_path(fs_std_data_path(filename))
}
fn test_fs_overlay_path(overlay_id: impl Display) -> PathBuf {
    test_path(fs_std_overlay_path(overlay_id))
}

fn rebuild_through_data_with_offset_table<T>(original_data: &[u8], new_data: impl Write)
where
    T: TryFrom<DataWithOffsetTable>,
    <T as TryFrom<DataWithOffsetTable>>::Error: Debug,
    T: TryInto<DataWithOffsetTable>,
    <T as TryInto<DataWithOffsetTable>>::Error: Debug,
{
    T::try_from(DataWithOffsetTable::from_reader(original_data).unwrap())
        .unwrap()
        .try_into()
        .unwrap()
        .to_writer(
            new_data,
            Some(STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT),
            true,
        )
        .unwrap();
}

#[rstest]
fn rebuild_data_with_offset_table_file(
    #[files("tests/data/data/**/*Mes*.dat")]
    #[files("tests/data/data/**/mfset_*.dat")]
    path: PathBuf,
) {
    let original_data = fs::read(path).unwrap();
    let mut new_data: Vec<u8> = Vec::new();

    DataWithOffsetTable::from_reader(&original_data[..])
        .unwrap()
        .to_writer(&mut new_data, None, true)
        .unwrap();

    assert_eq!(new_data, original_data);
}

#[rstest]
fn rebuild_field_maps() {
    let original_fmapdata = fs::read(test_fs_data_path("FMap/FMapData.dat")).unwrap();
    let original_treasure_info = fs::read(test_fs_data_path("Treasure/TreasureInfo.dat")).unwrap();
    let original_overlay3 = fs::read(test_fs_overlay_path(3)).unwrap();
    let original_overlay4 = fs::read(test_fs_overlay_path(4)).unwrap();

    let mut new_fmapdata: Vec<u8> = Vec::new();
    let mut new_treasure_info: Vec<u8> = Vec::new();
    let mut new_overlay3 = original_overlay3.clone();
    let mut new_overlay4 = original_overlay4.clone();

    FieldMaps::from_files(
        &original_fmapdata[..],
        &original_treasure_info[..],
        Cursor::new(&original_overlay3),
        Cursor::new(&original_overlay4),
    )
    .unwrap()
    .to_files(
        &mut new_fmapdata,
        &mut new_treasure_info,
        Cursor::new(&mut new_overlay3),
        Cursor::new(&mut new_overlay4),
        true,
    )
    .unwrap();

    assert_eq!(new_fmapdata, original_fmapdata);
    assert_eq!(new_treasure_info, original_treasure_info);
    assert_eq!(new_overlay3, original_overlay3);
    assert_eq!(new_overlay4, original_overlay4);
}

#[rstest]
#[ignore = "compression and decompression of all chunks is very slow"]
fn rebuild_field_maps_full() {
    let original_fmapdata = fs::read(test_fs_data_path("FMap/FMapData.dat")).unwrap();
    let original_treasure_info = fs::read(test_fs_data_path("Treasure/TreasureInfo.dat")).unwrap();
    let original_overlay3 = fs::read(test_fs_overlay_path(3)).unwrap();
    let original_overlay4 = fs::read(test_fs_overlay_path(4)).unwrap();

    let mut field_maps = FieldMaps::from_files(
        &original_fmapdata[..],
        &original_treasure_info[..],
        Cursor::new(&original_overlay3),
        Cursor::new(&original_overlay4),
    )
    .unwrap();

    for map in &field_maps.maps {
        let map_chunk = FieldMapChunk::try_from(
            DataWithOffsetTable::from_reader(Cursor::new(
                field_maps.fmapdata_chunks[map.map_chunk_index]
                    .to_uncompressed(true)
                    .unwrap(),
            ))
            .unwrap(),
        )
        .unwrap();
        for (i, &tileset_index) in map.tileset_indexes.iter().enumerate() {
            if let Some(tileset_index) = tileset_index {
                let pixel_format = map_chunk
                    .properties
                    .tilesets_properties
                    .tileset_pixel_formats()[i];
                field_maps.fmapdata_chunks[tileset_index] = MaybeCompressedData::Uncompressed(
                    Tileset::from_bytes(
                        &field_maps.fmapdata_chunks[tileset_index]
                            .to_uncompressed(true)
                            .unwrap(),
                        pixel_format,
                    )
                    .unwrap()
                    .to_bytes(pixel_format)
                    .unwrap(),
                );
            }
        }
        let mut map_chunk_data = Vec::new();
        DataWithOffsetTable::try_from(map_chunk)
            .unwrap()
            .to_writer(
                &mut map_chunk_data,
                Some(STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT),
                true,
            )
            .unwrap();
        field_maps.fmapdata_chunks[map.map_chunk_index] =
            MaybeCompressedData::Uncompressed(map_chunk_data);
    }

    let mut new_fmapdata: Vec<u8> = Vec::new();
    let mut new_treasure_info: Vec<u8> = Vec::new();
    let mut new_overlay3 = original_overlay3.clone();
    let mut new_overlay4 = original_overlay4.clone();
    field_maps
        .to_files(
            &mut new_fmapdata,
            &mut new_treasure_info,
            Cursor::new(&mut new_overlay3),
            Cursor::new(&mut new_overlay4),
            true,
        )
        .unwrap();

    assert_eq!(new_fmapdata, original_fmapdata);
    assert_eq!(new_treasure_info, original_treasure_info);
    assert_eq!(new_overlay3, original_overlay3);
    assert_eq!(new_overlay4, original_overlay4);
}

#[rstest]
fn rebuild_battle_map_file() {
    let original_data = fs::read(test_fs_data_path("BMap/BMap.dat")).unwrap();
    let mut new_data: Vec<u8> = Vec::new();

    rebuild_through_data_with_offset_table::<BattleMapFile>(&original_data, &mut new_data);

    assert_eq!(new_data, original_data);
}

#[rstest]
#[ignore = "compression and decompression of all tilesets is very slow"]
fn rebuild_battle_map_file_full() {
    let original_data = fs::read(test_fs_data_path("BMap/BMap.dat")).unwrap();

    let mut battle_map_file =
        BattleMapFile::try_from(DataWithOffsetTable::from_reader(&original_data[..]).unwrap())
            .unwrap();

    for map in battle_map_file.maps.iter_mut() {
        if let MaybeSerialized::Serialized(data) = &map.tileset {
            map.tileset =
                MaybeSerialized::Deserialized(BattleMap::deserialize_tileset(data).unwrap());
        } else {
            panic!("No tilesets should be deserialized by default");
        }
    }

    let mut new_data: Vec<u8> = Vec::new();
    DataWithOffsetTable::try_from(battle_map_file)
        .unwrap()
        .to_writer(
            &mut new_data,
            Some(STANDARD_DATA_WITH_OFFSET_TABLE_ALIGNMENT),
            true,
        )
        .unwrap();

    assert_eq!(new_data, original_data);
}
