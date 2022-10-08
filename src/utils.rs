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

#[inline]
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[inline]
pub fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[inline]
pub fn now_unix_mills() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

#[inline]
pub fn now_unix_micros() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros()
}

#[inline]
pub fn bytes_to_hex(data: &[u8]) -> String {
    hex::encode(data)
}

#[inline]
pub fn hex_to_bytes(data: &str) -> g::Result<Vec<u8>> {
    match hex::decode(data) {
        Err(_) => Err(g::Err::UtilsHexStr),
        Ok(v) => Ok(v),
    }
}

pub fn init_log_with_path(path: &str) {
    use tracing_appender::rolling;
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    assert!(path.len() > 0);

    let dfile = rolling::hourly(path, "DEBUG").with_max_level(tracing::Level::DEBUG);
    let wfile = rolling::hourly(path, "WARN").with_max_level(tracing::Level::WARN);
    let efile = rolling::hourly(path, "ERROR").with_max_level(tracing::Level::ERROR);
    let ifile = rolling::hourly(path, "INFO").with_max_level(tracing::Level::INFO);
    let af = dfile.and(wfile).and(efile).and(ifile);

    use time::macros::format_description;
    use time::UtcOffset;
    use tracing_subscriber::fmt::time::OffsetTime;

    let lev;
    #[cfg(debug_assertions)]
    {
        lev = tracing::Level::DEBUG;
    }
    #[cfg(not(debug_assertions))]
    {
        lev = tracing::Level::INFO;
    }

    tracing_subscriber::fmt()
        .with_timer(OffsetTime::new(
            UtcOffset::from_hms(8, 0, 0).unwrap(),
            format_description!("[hour]:[minute]:[second].[subsecond digits:6]"),
        ))
        .with_max_level(lev)
        .with_writer(af)
        .with_ansi(false)
        .init();
}


pub fn init_log() {
    use time::macros::format_description;
    use time::UtcOffset;
    use tracing_subscriber::fmt::time::OffsetTime;

    let lev;
    #[cfg(debug_assertions)]
    {
        lev = tracing::Level::DEBUG;
    }
    #[cfg(not(debug_assertions))]
    {
        lev = tracing::Level::INFO;
    }

    tracing_subscriber::fmt()
        .with_timer(OffsetTime::new(
            UtcOffset::from_hms(8, 0, 0).unwrap(),
            format_description!(
                "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:6]"
            ),
        ))
        .with_max_level(lev)
        .init();
}