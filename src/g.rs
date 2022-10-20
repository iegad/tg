use core::fmt;

/// Err tg库中的所有错误类型定义
#[derive(Debug)]
pub enum Err {
    /// 没有错误
    None,

    /// 未知错误, 未知错误必需携带附加说明.
    ///
    /// # Examples
    ///
    /// ```
    /// use tg::g::Err;
    /// use std::fs::File;
    /// use std::io::BufReader;
    /// use std::io::Read;
    ///
    /// if let Ok(f) = File::open("g.rs") {
    ///     let mut buf = vec![0u8; 1024];
    ///     let mut reader = BufReader::new(f);
    ///     if let Err(err) = reader.read(&mut buf) {
    ///         Err::Unknown(format!("{:?}", err));
    ///     }
    /// }
    /// ```
    Unknown(String),

    /// 自定义错误, 附加值为错误详情
    ///
    /// # Examples
    ///
    /// ```
    /// use tg::g::Err;
    ///
    /// println!("{}", Err::Custom("custom error".to_string()));
    /// ```
    Custom(String),

    // ---------------------------------------------- utils 错误定义 ----------------------------------------------
    //
    //
    /// utils::get_pwd 获取当前工作目录错误
    UtilsGetPW1,

    /// utils::get_pwd 当前工作目录的字符串中包含无效的Unicode字符
    UtilsGetPW2,

    /// 不是正确的16进制字符串
    UtilsHexStr,

    // ---------------------------------------------- nw 错误定义 ----------------------------------------------
    //
    //
    /// 无效的端口
    NwInvalidPort,
    
    /// 无效的IP
    NwInvalidIP,

    /// 无效的连接端
    ConnInvalid,

    /// Socket 通用错误
    SocketErr(String),

    /// 服务已开始运行
    ServerIsAlreadyRunning,

    // ---------------------------------------------- tcp 错误定义 ----------------------------------------------
    //
    //
    /// 无效的 endpoint, 附加值为无效的地址字符串
    ///
    /// 正确的 ip v4 的格式为: 192.168.1.100:9090, 127.0.0.1:9090, 0.0.0.0:9090
    ///
    /// 正确的 ip v6 的格式为: `[::]`:9090 ...
    ///
    /// # Examples
    ///
    /// ```
    /// use tg::g::Err;
    /// use std::net::SocketAddr;
    ///
    /// let s = "127.0.0.1:8080";
    /// if let Err(_) = s.parse::<SocketAddr>() {
    ///     panic!("{:?}", Err::TcpSocketAddrInvalid(s.to_string()));
    /// };
    /// ```
    TcpSocketAddrInvalid(String),

    /// tcp server accept 错误
    ServerAcceptError(String),

    /// TcpStream::peer_addr() 调用失败
    TcpGetRemoteAddrFailed,

    /// TcpStream::local_addr() 调用失败
    TcpGetLocalAddrFailed,

    /// tcp::Conn 读超时
    IOReadTimeout,

    IOWriteTimeout,

    /// TcpStream::read 调用失败, 附加值为错误详情
    IOReadFailed(String),

    /// TcpStream::write_all 调用失败, 附加值为错误详情
    IOWriteFailed(String),

    /// 设置套接字选项失败
    TcpSetOptionFailed(&'static str),

    TcpConnectFailed(String),

    // ---------------------------------------------- pack 错误定义 ----------------------------------------------
    //
    //
    /// 无效的消息头
    PackHeadInvalid,

    /// 无效的消息包
    PacketInvalid,

    /// 消息包数据太长
    PackTooLong,

    /// 缓冲区大小无效
    BufSizeInvalid,

    // ---------------------------------------------- piper 错误定义 ----------------------------------------------
    //
    //
    /// package id 无效
    PiperPIDInvalid,

    /// 已存在(package id)句柄
    PiperHandlerAreadyExists(u16),
}

/// 实现 println 的 Display特型
///
/// # Example
///
/// ```
/// use tg::g::Err;
///
/// let e = Err::Custom("test error".to_string());
/// println!("{}", e);
/// ```
impl fmt::Display for Err {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Err {}
unsafe impl Send for Err {}
unsafe impl Sync for Err {}

/// tg库 Result 类型定义
///
/// g::Result<T, g::Err>
pub type Result<T> = std::result::Result<T, Err>;

/// 默认监听地址
pub const DEFAULT_HOST: &str = "0.0.0.0:8080";
/// 最大连接数
pub const DEFAULT_MAX_CONNECTIONS: usize = 1000;
/// 管道默认长度: 1000
pub const DEFAULT_CHAN_SIZE: usize = 10000;
/// 默认读超时值 60秒
pub const DEFAULT_READ_TIMEOUT: u64 = 60;
/// 默认读缓冲区大小 1460
pub const DEFAULT_BUF_SIZE: usize = 4096;
