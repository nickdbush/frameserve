use std::sync::LazyLock;

use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::Deserialize;

static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    let config = Figment::new()
        .merge(Toml::file("fserve.toml"))
        .merge(Env::prefixed("FSERVE_"))
        .extract::<Config>();
    match config {
        Ok(config) => config,
        Err(err) => {
            panic!("CONFIG ERROR: {err}");
        }
    }
});

#[derive(Deserialize)]
pub struct Config {
    pub bind_address: String,
    pub base: String,
    pub media_base: String,
}

pub fn get_config() -> &'static Config {
    &*CONFIG
}
