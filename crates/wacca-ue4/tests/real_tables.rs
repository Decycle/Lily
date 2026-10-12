//! Tests over the real game tables; run with `WACCA_ASSETS=<assets dir> cargo test -- --ignored`.
#![allow(clippy::unwrap_used)] // tests may unwrap (CONTRIBUTING)

use std::path::PathBuf;

use wacca_ue4::{DataTable, Value, read_datatable};

fn table_dir() -> PathBuf {
    let assets =
        std::env::var_os("WACCA_ASSETS").expect("set WACCA_ASSETS to the assets directory");
    PathBuf::from(assets).join("Table")
}

fn load(name: &str) -> DataTable {
    let dir = table_dir();
    let ua = std::fs::read(dir.join(format!("{name}.uasset"))).unwrap();
    let ux = std::fs::read(dir.join(format!("{name}.uexp"))).unwrap();
    read_datatable(&ua, &ux).unwrap()
}

fn has_unparsed(v: &Value) -> bool {
    match v {
        Value::Unparsed { .. } => true,
        Value::Struct(f) => f.iter().any(|(_, v)| has_unparsed(v)),
        Value::Array(items) => items.iter().any(has_unparsed),
        Value::Map(pairs) => pairs
            .iter()
            .any(|(k, v)| has_unparsed(k) || has_unparsed(v)),
        _ => false,
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn all_tables_parse_cleanly() {
    let mut names: Vec<String> = std::fs::read_dir(table_dir())
        .unwrap()
        .filter_map(|e| {
            e.unwrap()
                .file_name()
                .to_str()?
                .strip_suffix(".uasset")
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    assert_eq!(names.len(), 103);
    for name in &names {
        let t = load(name);
        assert!(!t.row_struct.is_empty(), "{name}");
        assert!(
            !t.rows.iter().any(|(_, r)| has_unparsed(r)),
            "{name} has unparsed values"
        );
    }
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn judge_touch_marvelous_front_is_3_frames() {
    let t = load("JudgeParameterTable");
    let v = t.row("Touch").and_then(|r| r.field("MarvelousTimingFront"));
    assert_eq!(v, Some(&Value::Int(3)));
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn game_score_max_is_one_million() {
    let t = load("GameScoreTable");
    let v = t
        .row("Data")
        .and_then(|r| r.field("MaxScore"))
        .and_then(Value::as_i64);
    assert_eq!(v, Some(1_000_000));
}

#[test]
#[ignore = "needs WACCA_ASSETS"]
fn music_parameter_table_has_395_songs() {
    let t = load("MusicParameterTable");
    assert_eq!(t.row_struct, "MusicParameterTableData");
    assert_eq!(t.rows.len(), 395);
}
