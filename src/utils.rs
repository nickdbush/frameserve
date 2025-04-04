use std::path::PathBuf;

pub fn extract_vid(file: &str) -> u32 {
    let path = PathBuf::from(file);
    path.file_stem()
        .unwrap()
        .to_string_lossy()
        .parse::<u32>()
        .unwrap()
}
