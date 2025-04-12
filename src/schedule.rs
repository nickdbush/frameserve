use jiff::Timestamp;
use serde::Serialize;

use crate::duration::{Duration, StepSize};

#[derive(Debug, Clone, Serialize)]
pub struct Schedule {
    pub step: StepSize,
    pub start: Timestamp,
    pub duration: Duration,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub vid: u32,
    pub start: Duration,
    pub duration: Duration,
}
