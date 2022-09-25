use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct CommonRsp<T> {
    code: i32,

    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
}

impl<T> CommonRsp<T> {
    pub fn new() -> Self {
        Self {
            code: 0,
            error: None,
            data: None,
        }
    }
}

pub mod info;
pub mod kick;
pub mod run;
