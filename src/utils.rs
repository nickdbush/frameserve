use std::{cmp::Ordering, ops::Range, path::PathBuf};

pub fn extract_vid(file: &str) -> u32 {
    let path = PathBuf::from(file);
    path.file_stem()
        .unwrap()
        .to_string_lossy()
        .parse::<u32>()
        .unwrap()
}

pub fn range_compare<T: Ord>(value: T, range: &Range<T>) -> Ordering {
    if value < range.start {
        Ordering::Less
    } else if value >= range.end {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}
