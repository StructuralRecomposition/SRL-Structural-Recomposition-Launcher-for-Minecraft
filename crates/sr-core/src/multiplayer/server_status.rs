//! Minecraft Java Edition Server List Ping (status protocol).
//!
//! This module only reports what the server actually returns. It never treats
//! an open TCP port as a Minecraft server, and it never claims that a client
//! joined the game. `online-mode` is not part of the status response and is
//! therefore reported as undetectable instead of being guessed.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAX_PACKET_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_SAMPLE_PLAYERS: usize = 200;
const DEFAULT_PORT: u16 = 25565;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnlineMode {
    Enabled,
    Disabled,
    Unknown,
    Undetectable,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatusError {
    /// address | dns | connect | handshake | protocol | timeout | closed | response
    pub stage: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatusReport {
    pub ok: bool,
    pub address: String,
    pub host: String,
    pub port: u16,
    pub resolved_ips: Vec<String>,
    pub dns_ms: Option<u64>,
    pub connect_ms: Option<u64>,
    pub latency_ms: Option<u64>,
    pub protocol: Option<i32>,
    pub version_name: Option<String>,
    pub motd: Option<String>,
    pub online_players: Option<u64>,
    pub max_players: Option<u64>,
    pub sample_players: Vec<String>,
    pub favicon_present: bool,
    pub online_mode: OnlineMode,
    pub online_mode_reason: String,
    pub auth_isolation: OnlineMode,
    pub auth_isolation_reason: String,
    pub error: Option<ServerStatusError>,
}

impl Default for ServerStatusReport {
    fn default() -> Self {
        Self {
            ok: false,
            address: String::new(),
            host: String::new(),
            port: DEFAULT_PORT,
            resolved_ips: Vec::new(),
            dns_ms: None,
            connect_ms: None,
            latency_ms: None,
            protocol: None,
            version_name: None,
            motd: None,
            online_players: None,
            max_players: None,
            sample_players: Vec::new(),
            favicon_present: false,
            online_mode: OnlineMode::Undetectable,
            online_mode_reason:
                "Minecraft Server List Ping 不返回 online-mode；SRL 不会根据 MOTD 或人数猜测。"
                    .into(),
            auth_isolation: OnlineMode::Unknown,
            auth_isolation_reason:
                "52FRP 开放 API 没有提供“正版隔离”字段；SRL 不会把 TCP 隧道状态当作正版验证结果。"
                    .into(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub host: String,
    pub port: u16,
}

impl ServerAddress {
    pub fn display(&self) -> String {
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{host}:{}", self.port)
    }
}

/// Accept `host`, `host:port`, IPv4, `[IPv6]:port`, and raw IPv6 literals.
/// A bare host uses the Java Edition default port 25565.
pub fn parse_address(raw: &str) -> Result<ServerAddress, ServerStatusError> {
    let mut value = raw.trim();
    if let Some(rest) = value.strip_prefix("minecraft://") {
        value = rest.trim_end_matches('/');
    }
    if value.is_empty() {
        return Err(error("address", "请输入服务器地址", false));
    }
    if value.len() > 300 {
        return Err(error("address", "服务器地址过长", false));
    }
    if value.chars().any(|c| c.is_whitespace()) {
        return Err(error("address", "服务器地址不能包含空格", false));
    }
    if value.contains("://") {
        return Err(error(
            "address",
            "只接受 host:port，不要包含协议前缀",
            false,
        ));
    }

    if let Some(rest) = value.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| error("address", "IPv6 地址缺少右方括号", false))?;
        let port = match tail.strip_prefix(':') {
            Some(port) => parse_port(port)?,
            None if tail.is_empty() => DEFAULT_PORT,
            None => return Err(error("address", "IPv6 地址后的端口格式无效", false)),
        };
        validate_host(host)?;
        return Ok(ServerAddress {
            host: host.to_string(),
            port,
        });
    }

    let colons = value.matches(':').count();
    if colons > 1 {
        // Raw IPv6 literal without brackets and without a port. Anything else
        // with several colons is malformed, not a hostname.
        if value.parse::<IpAddr>().is_err() {
            return Err(error("address", "IPv6 地址格式无效", false));
        }
        return Ok(ServerAddress {
            host: value.to_string(),
            port: DEFAULT_PORT,
        });
    }
    let (host, port) = match value.rsplit_once(':') {
        Some((host, port)) => (host, parse_port(port)?),
        None => (value, DEFAULT_PORT),
    };
    validate_host(host)?;
    Ok(ServerAddress {
        host: host.to_string(),
        port,
    })
}

fn parse_port(value: &str) -> Result<u16, ServerStatusError> {
    if value.is_empty() {
        return Err(error("address", "端口不能为空", false));
    }
    let port = value
        .parse::<u16>()
        .map_err(|_| error("address", "端口必须是 1–65535 的整数", false))?;
    if port == 0 {
        return Err(error("address", "端口必须大于 0", false));
    }
    Ok(port)
}

fn validate_host(host: &str) -> Result<(), ServerStatusError> {
    if host.is_empty() || host.len() > 253 {
        return Err(error("address", "主机名长度无效", false));
    }
    if host.contains('/') || host.contains('\\') || host.contains('@') {
        return Err(error("address", "主机名包含非法字符", false));
    }
    if host.starts_with('.') || host.ends_with('.') {
        return Err(error("address", "主机名不能以点开头或结尾", false));
    }
    Ok(())
}

fn error(stage: &str, message: impl Into<String>, retryable: bool) -> ServerStatusError {
    ServerStatusError {
        stage: stage.into(),
        message: message.into(),
        retryable,
    }
}

pub async fn ping_server(address: &str) -> ServerStatusReport {
    ping_with_timeout(address, Duration::from_secs(12)).await
}

pub async fn ping_with_timeout(address: &str, total: Duration) -> ServerStatusReport {
    let parsed = match parse_address(address) {
        Ok(value) => value,
        Err(error) => {
            return ServerStatusReport {
                address: address.trim().to_string(),
                error: Some(error),
                ..ServerStatusReport::default()
            }
        }
    };
    let display = parsed.display();
    match tokio::time::timeout(total, ping_parsed(parsed, total)).await {
        Ok(report) => report,
        Err(_) => ServerStatusReport {
            address: display,
            error: Some(error("timeout", "服务器状态查询总超时", true)),
            ..ServerStatusReport::default()
        },
    }
}

async fn ping_parsed(parsed: ServerAddress, total: Duration) -> ServerStatusReport {
    let mut report = ServerStatusReport {
        address: parsed.display(),
        host: parsed.host.clone(),
        port: parsed.port,
        ..ServerStatusReport::default()
    };

    let dns_started = Instant::now();
    let addrs = match resolve(&parsed.host, parsed.port).await {
        Ok(values) if !values.is_empty() => values,
        Ok(_) => {
            report.error = Some(error("dns", "DNS 没有返回可用地址", true));
            return report;
        }
        Err(failure) => {
            report.error = Some(failure);
            return report;
        }
    };
    report.dns_ms = Some(dns_started.elapsed().as_millis() as u64);
    report.resolved_ips = addrs
        .iter()
        .map(|addr| addr.ip().to_string())
        .collect::<Vec<_>>();
    report.resolved_ips.sort();
    report.resolved_ips.dedup();

    let connect_started = Instant::now();
    let mut stream = match connect_any(&addrs, total).await {
        Ok(stream) => stream,
        Err(failure) => {
            report.error = Some(failure);
            return report;
        }
    };
    report.connect_ms = Some(connect_started.elapsed().as_millis() as u64);

    let ping_started = Instant::now();
    let result = async {
        let mut handshake = Vec::new();
        write_var_int(&mut handshake, 0x00);
        write_var_int(&mut handshake, -1);
        write_string(&mut handshake, &parsed.host)?;
        handshake.extend_from_slice(&parsed.port.to_be_bytes());
        write_var_int(&mut handshake, 1);
        write_frame(&mut stream, &handshake).await?;
        write_frame(&mut stream, &[0x00]).await?;

        let packet = read_frame(&mut stream).await?;
        let mut reader = PacketReader::new(&packet);
        let packet_id = reader.var_int()?;
        if packet_id != 0 {
            return Err(error("protocol", "服务器返回了非状态数据包", false));
        }
        let json = reader.string(MAX_JSON_BYTES)?;
        let value: serde_json::Value = serde_json::from_str(&json)
            .map_err(|_| error("response", "服务器状态不是有效的 JSON", false))?;
        if !value.is_object() {
            return Err(error("response", "服务器状态 JSON 结构无效", false));
        }
        Ok::<_, ServerStatusError>(value)
    }
    .await;

    let value = match result {
        Ok(value) => value,
        Err(failure) => {
            report.error = Some(failure);
            return report;
        }
    };
    report.latency_ms = Some(ping_started.elapsed().as_millis() as u64);
    report.protocol = value
        .get("version")
        .and_then(|v| v.get("protocol"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    report.version_name = value
        .get("version")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    report.motd = value
        .get("description")
        .map(json_text)
        .filter(|v| !v.is_empty());
    report.online_players = value
        .get("players")
        .and_then(|v| v.get("online"))
        .and_then(|v| v.as_u64());
    report.max_players = value
        .get("players")
        .and_then(|v| v.get("max"))
        .and_then(|v| v.as_u64());
    report.sample_players = value
        .get("players")
        .and_then(|v| v.get("sample"))
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("name").and_then(|v| v.as_str()))
                .take(MAX_SAMPLE_PLAYERS)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    report.favicon_present = value
        .get("favicon")
        .and_then(|v| v.as_str())
        .is_some_and(|v| !v.is_empty());
    report.ok = true;
    report
}

async fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, ServerStatusError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    match tokio::time::timeout(
        Duration::from_secs(6),
        tokio::net::lookup_host((host, port)),
    )
    .await
    {
        Err(_) => Err(error("dns", "DNS 解析超时", true)),
        Ok(Err(failure)) => Err(error("dns", format!("DNS 解析失败：{failure}"), true)),
        Ok(Ok(iter)) => Ok(iter.collect()),
    }
}

async fn connect_any(
    addrs: &[SocketAddr],
    total: Duration,
) -> Result<TcpStream, ServerStatusError> {
    let started = Instant::now();
    let mut last: Option<std::io::Error> = None;
    for addr in addrs {
        let remaining = total.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let budget = remaining.min(Duration::from_secs(5));
        match tokio::time::timeout(budget, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Ok(Err(failure)) => last = Some(failure),
            Err(_) => {
                return Err(error(
                    "timeout",
                    format!("连接 {addr} 超时（{} 秒）", budget.as_secs_f32()),
                    true,
                ))
            }
        }
    }
    Err(match last {
        Some(failure) if failure.kind() == std::io::ErrorKind::ConnectionRefused => {
            error("connect", "端口拒绝连接：服务器可能未启动或端口错误", true)
        }
        Some(failure) => error("connect", format!("无法连接服务器：{failure}"), true),
        None => error("connect", "没有可尝试的服务器地址", true),
    })
}

fn write_var_int(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), ServerStatusError> {
    if value.len() > 32767 {
        return Err(error("handshake", "服务器地址过长", false));
    }
    write_var_int(out, value.len() as i32);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

async fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<(), ServerStatusError> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    write_var_int(&mut frame, payload.len() as i32);
    frame.extend_from_slice(payload);
    stream
        .write_all(&frame)
        .await
        .map_err(|failure| error("handshake", format!("发送握手失败：{failure}"), true))
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, ServerStatusError> {
    let mut length = 0u32;
    let mut shift = 0;
    for _ in 0..5 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await.map_err(|failure| {
            if failure.kind() == std::io::ErrorKind::UnexpectedEof {
                error(
                    "closed",
                    "服务器关闭了连接（可能不是 Minecraft 服务器）",
                    false,
                )
            } else {
                error("handshake", format!("读取响应失败：{failure}"), true)
            }
        })?;
        length |= ((byte[0] & 0x7f) as u32) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(error("protocol", "数据包长度 VarInt 非法", false));
        }
    }
    let length = length as usize;
    if length == 0 {
        return Err(error("protocol", "服务器返回了空数据包", false));
    }
    if length > MAX_PACKET_BYTES {
        return Err(error("protocol", "服务器响应超过 4 MB，已拒绝", false));
    }
    let mut payload = vec![0u8; length];
    tokio::time::timeout(Duration::from_secs(8), stream.read_exact(&mut payload))
        .await
        .map_err(|_| error("timeout", "等待服务器状态响应超时", true))?
        .map_err(|failure| error("closed", format!("读取服务器响应中断：{failure}"), false))?;
    Ok(payload)
}

struct PacketReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> PacketReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn var_int(&mut self) -> Result<i32, ServerStatusError> {
        let mut value = 0u32;
        let mut shift = 0;
        for _ in 0..5 {
            let byte = *self
                .data
                .get(self.position)
                .ok_or_else(|| error("protocol", "VarInt 数据不完整", false))?;
            self.position += 1;
            value |= ((byte & 0x7f) as u32) << shift;
            if byte & 0x80 == 0 {
                return Ok(value as i32);
            }
            shift += 7;
        }
        Err(error("protocol", "VarInt 超过 5 字节", false))
    }

    fn string(&mut self, max: usize) -> Result<String, ServerStatusError> {
        let length = self.var_int()?;
        if length < 0 {
            return Err(error("protocol", "字符串长度非法", false));
        }
        let length = length as usize;
        if length > max {
            return Err(error("protocol", "服务器字符串超过大小上限", false));
        }
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| error("protocol", "字符串长度超出数据边界", false))?;
        let bytes = &self.data[self.position..end];
        self.position = end;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| error("protocol", "服务器字符串不是有效的 UTF-8", false))
    }
}

/// Flatten a Minecraft chat component into readable text.
pub fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts.iter().map(json_text).collect::<Vec<_>>().join(""),
        serde_json::Value::Object(map) => {
            let mut out = String::new();
            if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
            if let Some(translate) = map.get("translate").and_then(|v| v.as_str()) {
                if out.is_empty() {
                    out.push_str(translate);
                }
            }
            if let Some(extra) = map.get("extra") {
                out.push_str(&json_text(extra));
            }
            out
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn parses_addresses() {
        assert_eq!(
            parse_address("example.com").unwrap(),
            ServerAddress {
                host: "example.com".into(),
                port: 25565
            }
        );
        assert_eq!(
            parse_address("example.com:25566").unwrap(),
            ServerAddress {
                host: "example.com".into(),
                port: 25566
            }
        );
        assert_eq!(
            parse_address("127.0.0.1:25565").unwrap(),
            ServerAddress {
                host: "127.0.0.1".into(),
                port: 25565
            }
        );
        assert_eq!(
            parse_address("[::1]:25565").unwrap(),
            ServerAddress {
                host: "::1".into(),
                port: 25565
            }
        );
        assert_eq!(
            parse_address("2001:db8::1").unwrap(),
            ServerAddress {
                host: "2001:db8::1".into(),
                port: 25565
            }
        );
        assert!(parse_address("").is_err());
        assert!(parse_address("host:0").is_err());
        assert!(parse_address("a b:25565").is_err());
        assert!(parse_address("[::1:25565").is_err());
    }

    #[test]
    fn varint_roundtrip() {
        for value in [0, 1, 127, 128, 255, 2147483647, -1] {
            let mut out = Vec::new();
            write_var_int(&mut out, value);
            let mut reader = PacketReader::new(&out);
            assert_eq!(reader.var_int().unwrap(), value);
        }
    }

    #[test]
    fn rejects_oversized_string_lengths() {
        let data = [0xff, 0xff, 0x7f];
        let mut reader = PacketReader::new(&data);
        assert!(reader.string(1024).is_err());
    }

    #[test]
    fn flattens_chat_components() {
        let value = serde_json::json!({
            "text": "Hello ",
            "extra": [
                {"text": "world"},
                {"translate": "chat.type.text"}
            ]
        });
        assert_eq!(json_text(&value), "Hello worldchat.type.text");
    }

    fn varint(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        write_var_int(&mut out, value);
        out
    }

    async fn read_varint_async(stream: &mut TcpStream) -> i32 {
        let mut value = 0u32;
        let mut shift = 0;
        loop {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).await.unwrap();
            value |= ((byte[0] & 0x7f) as u32) << shift;
            if byte[0] & 0x80 == 0 {
                return value as i32;
            }
            shift += 7;
        }
    }

    #[tokio::test]
    async fn pings_a_mock_server_and_reads_status() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let handshake_len = read_varint_async(&mut stream).await as usize;
            let mut handshake = vec![0u8; handshake_len];
            stream.read_exact(&mut handshake).await.unwrap();
            let request_len = read_varint_async(&mut stream).await as usize;
            let mut request = vec![0u8; request_len];
            stream.read_exact(&mut request).await.unwrap();
            let status = serde_json::json!({
                "version": {"name": "1.21.1", "protocol": 767},
                "players": {"online": 2, "max": 20, "sample": [{"name": "Steve", "id": "x"}]},
                "description": {"text": "Test ", "extra": [{"text": "Server"}]},
                "favicon": "data:image/png;base64,AA=="
            })
            .to_string();
            let mut payload = varint(0x00);
            payload.extend(varint(status.len() as i32));
            payload.extend_from_slice(status.as_bytes());
            let mut frame = varint(payload.len() as i32);
            frame.extend(payload);
            stream.write_all(&frame).await.unwrap();
        });
        let report = ping_with_timeout(
            &format!("127.0.0.1:{}", addr.port()),
            Duration::from_secs(5),
        )
        .await;
        server.await.unwrap();
        assert!(report.ok, "{:?}", report.error);
        assert_eq!(report.version_name.as_deref(), Some("1.21.1"));
        assert_eq!(report.protocol, Some(767));
        assert_eq!(report.online_players, Some(2));
        assert_eq!(report.max_players, Some(20));
        assert_eq!(report.sample_players, vec!["Steve"]);
        assert_eq!(report.motd.as_deref(), Some("Test Server"));
        assert!(report.favicon_present);
        assert!(report.latency_ms.is_some());
        assert_eq!(report.online_mode, OnlineMode::Undetectable);
    }

    #[tokio::test]
    async fn reports_non_minecraft_server_honestly() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let response = b"HTTP/1.1 400 Bad Request\r\n\r\n";
            let _ = stream.write_all(response).await;
        });
        let report = ping_with_timeout(
            &format!("127.0.0.1:{}", addr.port()),
            Duration::from_secs(3),
        )
        .await;
        server.await.unwrap();
        assert!(!report.ok);
        assert!(report.error.is_some());
        assert!(!report.error.unwrap().message.is_empty());
    }

    #[tokio::test]
    async fn reports_connection_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let report = ping_with_timeout(&format!("127.0.0.1:{port}"), Duration::from_secs(2)).await;
        assert!(!report.ok);
        assert_eq!(report.error.unwrap().stage, "connect");
    }

    #[tokio::test]
    async fn reports_timeout_without_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(3)).await;
        });
        let started = Instant::now();
        let report = ping_with_timeout(
            &format!("127.0.0.1:{}", addr.port()),
            Duration::from_millis(500),
        )
        .await;
        assert!(!report.ok);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(report.error.is_some());
        server.abort();
    }
}
