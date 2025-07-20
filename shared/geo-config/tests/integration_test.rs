use std::path::PathBuf; 

use geo_config::Config;
use geo_overturemaps::GersId;

#[test]
fn test_config_reading() {
    let path = PathBuf::from("tests/data/config_example.toml");
    let config = Config::read_from_file(&path).expect("Failed to read config file");
    assert_eq!(config.overturemaps.gers_id, GersId::new("dbd84987-2831-4b62-a0e0-a3f3d5a237c2".to_string()));
}