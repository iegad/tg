use crate::g;
use std::time::SystemTime;

pub fn get_pwd() -> g::Result<String> {
    let path = match std::env::current_dir() {
        Err(_) => {
            return Err(g::Err::UtilsGetPW1);
        }
        Ok(p) => p,
    };

    let result = match path.into_os_string().into_string() {
        Err(_) => {
            return Err(g::Err::UtilsGetPW2);
        }
        Ok(v) => v,
    };

    Ok(result)
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

pub fn now_unix_mills() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

pub fn now_unix_micros() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros()
}
