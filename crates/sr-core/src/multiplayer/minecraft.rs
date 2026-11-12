use crate::instance::Instance;
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::types::{Edition, LocalServiceView};

/// Minecraft writes this line when the player opens a LAN world. The port is
/// allocated dynamically, so it must never be assumed to be 25565.
pub fn parse_lan_port(text: &str) -> Option<u16> {
    const MARKERS: &[&str] = &[
        "Started serving on ",
        "Local game hosted on port ",
        "Opening LAN on port ",
        "本地游戏已在端口 ",
    ];
    for line in text.lines().rev() {
        for marker in MARKERS {
            if let Some(rest) = line.split_once(marker).map(|(_, rest)| rest) {
                let digits: String = rest
                    .chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(port) = digits.parse::<u16>() {
                    if port > 0 {
                        return Some(port);
                    }
                }
            }
        }
    }
    None
}

pub fn parse_lsof_ports(output: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in output.lines() {
        let Some(addr) = line.strip_prefix('n') else {
            continue;
        };
        let addr = addr.split("->").next().unwrap_or(addr);
        let Some((_, port)) = addr.rsplit_once(':') else {
            continue;
        };
        let port = port.trim_matches(|c| c == ']' || c == ' ');
        if let Ok(port) = port.parse::<u16>() {
            if port > 0 {
                ports.push(port);
            }
        }
    }
    dedup(ports)
}

pub fn parse_ss_ports(output: &str, pid: u32) -> Vec<u16> {
    let needle = format!("pid={pid}");
    let mut ports = Vec::new();
    for line in output.lines() {
        if !line.contains(&needle) {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let local = parts[3];
        if let Some((_, port)) = local.rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                if port > 0 {
                    ports.push(port);
                }
            }
        }
    }
    dedup(ports)
}

pub fn parse_netstat_ports(output: &str, pid: u32) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        if parts.last().and_then(|v| v.parse::<u32>().ok()) != Some(pid) {
            continue;
        }
        let local = parts[1];
        if let Some((_, port)) = local.rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                if port > 0 {
                    ports.push(port);
                }
            }
        }
    }
    dedup(ports)
}

fn dedup(mut ports: Vec<u16>) -> Vec<u16> {
    ports.sort_unstable();
    ports.dedup();
    ports
}

pub async fn detect_local(
    instance: &Instance,
    pid: Option<u32>,
    edition: Edition,
    logs_dir: &Path,
) -> LocalServiceView {
    let log_port = latest_instance_log(logs_dir, &instance.id)
        .and_then(|path| read_log_tail(&path, 512 * 1024))
        .and_then(|text| parse_lan_port(&text));

    let ports = match pid {
        Some(pid) => listening_ports(pid, edition).await,
        None => Vec::new(),
    };

    if edition == Edition::Java {
        if let Some(port) = log_port {
            return LocalServiceView {
                found: true,
                pid,
                edition,
                ports: if ports.is_empty() { vec![port] } else { ports },
                detected_port: Some(port),
                source: "log".into(),
                message: format!("从游戏日志识别到局域网端口 {port}"),
            };
        }
    }

    if ports.is_empty() {
        return LocalServiceView {
            found: false,
            pid,
            edition,
            ports: Vec::new(),
            detected_port: None,
            source: "none".into(),
            message: if pid.is_none() {
                "没有检测到由 SRL 启动的游戏进程；请先启动游戏，或手动填写本地端口。".into()
            } else {
                format!(
                    "未发现正在监听的 {} 端口；请先在游戏内开放局域网，或手动填写端口。",
                    edition.tunnel_type()
                )
            },
        };
    }

    let preferred = edition.default_port();
    let port = if ports.contains(&preferred) {
        Some(preferred)
    } else {
        ports.first().copied()
    };
    LocalServiceView {
        found: true,
        pid,
        edition,
        ports,
        detected_port: port,
        source: "process".into(),
        message: match pid {
            Some(pid) => format!("从进程 {pid} 的监听端口识别到 {}", edition.label()),
            None => "从本地监听端口识别到游戏服务".into(),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerEvent {
    pub player: String,
    /// join | leave
    pub action: String,
    pub at: Option<String>,
    pub observed_ip: Option<String>,
    /// loopback | lan | public | unknown
    pub ip_kind: String,
    pub source: String,
    /// 1-based line number inside the captured log file. Used for stable
    /// de-duplication when a line has no parseable timestamp.
    #[serde(skip)]
    pub line: usize,
}

/// Classify an address observed by the Minecraft server. A loopback address
/// proves a local/proxy connection, never a public visitor IP.
pub fn classify_ip(ip: &str) -> &'static str {
    if let Ok(addr) = ip.parse::<std::net::IpAddr>() {
        match addr {
            std::net::IpAddr::V4(v4) => {
                if v4.is_loopback() {
                    "loopback"
                } else if v4.is_private() || v4.is_link_local() {
                    "lan"
                } else {
                    "public"
                }
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback() {
                    "loopback"
                } else if v6.is_unique_local() || v6.is_unicast_link_local() {
                    "lan"
                } else {
                    "public"
                }
            }
        }
    } else {
        "unknown"
    }
}

fn valid_player_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && c != '<' && c != '>')
}

/// SRL wraps every captured stdout/stderr line as `[stdout] ...`.
/// Parsing must look past that wrapper before reading Minecraft's own
/// `[HH:MM:SS]` prefix.
fn strip_stream_prefix(line: &str) -> &str {
    let mut current = line.trim_start();
    loop {
        if let Some(rest) = current.strip_prefix("[stdout]") {
            current = rest.trim_start();
        } else if let Some(rest) = current.strip_prefix("[stderr]") {
            current = rest.trim_start();
        } else {
            return current;
        }
    }
}

fn line_clock(line: &str) -> Option<NaiveTime> {
    let line = strip_stream_prefix(line);
    let start = line.find('[')?;
    let end = line[start + 1..].find(']')? + start + 1;
    NaiveTime::parse_from_str(&line[start + 1..end], "%H:%M:%S").ok()
}

fn message_of(line: &str) -> Option<&str> {
    // [12:34:56] [Server thread/INFO]: message (possibly wrapped by SRL)
    let line = strip_stream_prefix(line);
    let first = line.find("]: ")?;
    Some(&line[first + 3..])
}

/// Parse vanilla/integrated-server player events. Only lines that contain a
/// real join/leave/login marker are used; chat text is ignored.
pub fn parse_player_events(text: &str, date: Option<NaiveDate>) -> Vec<PlayerEvent> {
    let mut events = Vec::new();
    let mut current_date = date;
    let mut previous_time: Option<NaiveTime> = None;
    for (line_no, line) in text.lines().enumerate() {
        let Some(message) = message_of(line) else {
            continue;
        };
        let clock = line_clock(line);
        if let (Some(previous), Some(current)) = (previous_time, clock) {
            // A large backwards jump is a midnight rollover, not a clock
            // rollback. Small rollbacks are left on the same date and marked
            // by the caller as low-confidence if needed.
            if previous.signed_duration_since(current) > chrono::Duration::hours(12) {
                if let Some(next) = current_date.and_then(|value| value.succ_opt()) {
                    current_date = Some(next);
                }
            }
        }
        if let Some(clock) = clock {
            previous_time = Some(clock);
        }
        let at = current_date.zip(clock).and_then(|(date, time)| {
            chrono::Local
                .from_local_datetime(&date.and_time(time))
                .single()
                .map(|local| local.with_timezone(&Utc).to_rfc3339())
        });

        if let Some(position) = message.find(" logged in with entity id") {
            let head = message[..position].trim();
            let (name, ip) = match head.split_once('[') {
                Some((name, rest)) => {
                    let rest = rest.strip_prefix('/').unwrap_or(rest);
                    let ip = rest
                        .split_once(']')
                        .map(|(ip, _)| ip)
                        .unwrap_or(rest)
                        .rsplit_once(':')
                        .map(|(ip, port)| {
                            if port.chars().all(|c| c.is_ascii_digit()) {
                                ip
                            } else {
                                rest.trim_end_matches(']')
                            }
                        })
                        .unwrap_or(rest)
                        .to_string();
                    (name.trim(), Some(ip))
                }
                None => (head, None),
            };
            let name = name.strip_prefix("Player ").unwrap_or(name);
            if valid_player_name(name) {
                let ip_kind = ip
                    .as_deref()
                    .map(classify_ip)
                    .unwrap_or("unknown")
                    .to_string();
                events.push(PlayerEvent {
                    player: name.to_string(),
                    action: "join".into(),
                    at: at.clone(),
                    observed_ip: ip,
                    ip_kind,
                    source: "minecraft-log".into(),
                    line: line_no + 1,
                });
            }
            continue;
        }

        for (marker, action) in [
            (" joined the game", "join"),
            (" left the game", "leave"),
            (" lost connection:", "leave"),
        ] {
            if let Some(position) = message.find(marker) {
                let name = message[..position].trim();
                let name = name.strip_prefix("Player ").unwrap_or(name);
                if valid_player_name(name) {
                    events.push(PlayerEvent {
                        player: name.to_string(),
                        action: action.into(),
                        at: at.clone(),
                        observed_ip: None,
                        ip_kind: "unknown".into(),
                        source: "minecraft-log".into(),
                        line: line_no + 1,
                    });
                }
                break;
            }
        }
    }
    events
}

/// Read player events from the newest SRL-captured instance log. `None` means
/// no log exists; an empty vector means the log was read but had no events.
pub fn read_player_events(
    logs_dir: &Path,
    instance_id: &str,
    since: Option<DateTime<Utc>>,
) -> Option<Vec<PlayerEvent>> {
    let path = latest_instance_log(logs_dir, instance_id)?;
    let date = log_date(&path);
    let text = read_log_tail(&path, 4 * 1024 * 1024)?;
    let mut events = parse_player_events(&text, date);
    if let Some(since) = since {
        events.retain(|event| match event.at.as_deref() {
            Some(at) => DateTime::parse_from_rfc3339(at)
                .map(|value| value.with_timezone(&Utc) >= since)
                .unwrap_or(true),
            None => true,
        });
    }
    Some(events)
}

fn log_date(path: &Path) -> Option<NaiveDate> {
    let name = path.file_name()?.to_string_lossy();
    let stamp = name.split('-').rev().take(2).collect::<Vec<_>>();
    // file name: <instance>-YYYYMMDD-HHMMSS.log
    let stamp = format!(
        "{}-{}",
        stamp.get(1)?,
        stamp.first()?.trim_end_matches(".log")
    );
    chrono::NaiveDateTime::parse_from_str(&stamp, "%Y%m%d-%H%M%S")
        .ok()
        .map(|value| value.date())
}

fn latest_instance_log(logs_dir: &Path, instance_id: &str) -> Option<PathBuf> {
    let dir = logs_dir.join("instances");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("{instance_id}-")) && name.ends_with(".log")
                })
        })
        .collect();
    candidates.sort();
    candidates.pop()
}

fn read_log_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(max_bytes).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

async fn listening_ports(pid: u32, edition: Edition) -> Vec<u16> {
    #[cfg(unix)]
    {
        let tcp_flag = if edition == Edition::Java {
            "-iTCP"
        } else {
            "-iUDP"
        };
        let lsof = run_command("lsof", &["-nP", "-p", &pid.to_string(), tcp_flag, "-Fn"]).await;
        if let Some(output) = lsof {
            let ports = parse_lsof_ports(&output);
            if !ports.is_empty() {
                return ports;
            }
            // lsof can be installed but restricted by permissions. Fall
            // through to ss instead of reporting "no ports".
        }

        let flag = if edition == Edition::Java {
            "-ltnp"
        } else {
            "-lunp"
        };
        if let Some(output) = run_command("ss", &["-H", flag]).await {
            let ports = parse_ss_ports(&output, pid);
            if !ports.is_empty() {
                return ports;
            }
        }
    }

    #[cfg(windows)]
    {
        let protocol = if edition == Edition::Java {
            "tcp"
        } else {
            "udp"
        };
        if let Some(output) = run_command("netstat", &["-ano", "-p", protocol]).await {
            return parse_netstat_ports(&output, pid);
        }
    }

    let _ = pid;
    let _ = edition;
    Vec::new()
}

async fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(program).args(args).output(),
    )
    .await
    .ok()?
    .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A TCP connect is the cheapest real proof that the local service accepts
/// connections. UDP has no connect handshake; the detector relies on the
/// listening-port table instead.
/// Java Edition health check: a real Server List Ping, not just an open port.
pub async fn java_protocol_ok(ip: &str, port: u16) -> bool {
    let host = if ip.contains(':') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    };
    let report = super::server_status::ping_with_timeout(&host, Duration::from_secs(3)).await;
    report.ok
}

const RAKNET_MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];

/// Validate a RakNet Unconnected Pong. Only the first byte is not enough:
/// magic, timestamp/guid width, MOTD length and UTF-8 must all be sane.
pub fn valid_raknet_pong(buf: &[u8]) -> bool {
    if buf.len() < 35 || buf[0] != 0x1c {
        return false;
    }
    if buf[17..33] != RAKNET_MAGIC {
        return false;
    }
    let length = u16::from_be_bytes([buf[33], buf[34]]) as usize;
    let end = 35usize.saturating_add(length);
    if end > buf.len() || length == 0 {
        return false;
    }
    std::str::from_utf8(&buf[35..end]).is_ok()
}

/// Bedrock Edition health check using RakNet Unconnected Ping.
pub async fn bedrock_protocol_ok(ip: &str, port: u16) -> bool {
    let Ok(socket) = tokio::net::UdpSocket::bind(("0.0.0.0", 0)).await else {
        return false;
    };
    let mut packet = Vec::with_capacity(33);
    packet.push(0x01);
    packet.extend_from_slice(&chrono::Utc::now().timestamp_millis().to_be_bytes());
    packet.extend_from_slice(&RAKNET_MAGIC);
    packet.extend_from_slice(&(std::process::id() as i64).to_be_bytes());
    let target = format!("{ip}:{port}");
    if socket.send_to(&packet, &target).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 2048];
    match tokio::time::timeout(Duration::from_secs(2), socket.recv_from(&mut buf)).await {
        Ok(Ok((len, source))) => {
            let expected: Option<std::net::IpAddr> = ip.parse().ok();
            if let Some(expected) = expected {
                if source.ip() != expected {
                    return false;
                }
            }
            valid_raknet_pong(&buf[..len])
        }
        _ => false,
    }
}

pub async fn tcp_reachable(ip: &str, port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_millis(1500),
        tokio::net::TcpStream::connect((ip, port)),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

/// UDP has no connect handshake. Binding the port is a practical local check:
/// if it is already occupied, another process (usually the game) is listening.
pub fn udp_port_in_use(port: u16) -> bool {
    match std::net::UdpSocket::bind(("0.0.0.0", port)) {
        Ok(socket) => {
            drop(socket);
            false
        }
        Err(error) => error.kind() == std::io::ErrorKind::AddrInUse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dynamic_lan_port_from_real_log_line() {
        let text = "[12:00:00] [Render thread/INFO]: Started serving on 64620\n";
        assert_eq!(parse_lan_port(text), Some(64620));
    }

    #[test]
    fn finds_legacy_lan_port_line() {
        let text = "Local game hosted on port 51234\n";
        assert_eq!(parse_lan_port(text), Some(51234));
    }

    #[test]
    fn parses_lsof_socket_names() {
        let output = "p123\nn127.0.0.1:25565\nn[::1]:25565\nn127.0.0.1:64620\n";
        assert_eq!(parse_lsof_ports(output), vec![25565, 64620]);
    }

    #[test]
    fn parses_ss_rows_for_pid() {
        let output = "LISTEN 0 128 0.0.0.0:25565 0.0.0.0:* users:((\"java\",pid=123,fd=20))\n\
                      LISTEN 0 128 0.0.0.0:64620 0.0.0.0:* users:((\"java\",pid=123,fd=21))\n\
                      LISTEN 0 128 0.0.0.0:8080 0.0.0.0:* users:((\"other\",pid=999,fd=21))\n";
        assert_eq!(parse_ss_ports(output, 123), vec![25565, 64620]);
    }

    #[test]
    fn detects_udp_port_state() {
        // Port 0 always binds; a real listener on a fixed port is covered by
        // the integration path, not this pure test.
        assert!(!udp_port_in_use(0));
    }

    #[test]
    fn parses_player_join_leave_and_observed_loopback_ip() {
        let text = "\
[12:00:00] [Server thread/INFO]: Steve joined the game\n\
[12:00:01] [Server thread/INFO]: Alex[/127.0.0.1:51234] logged in with entity id 7 at (0, 64, 0)\n\
[12:00:05] [Server thread/INFO]: <Steve> hello joined the game\n\
[12:01:00] [Server thread/INFO]: Alex left the game\n";
        let events = parse_player_events(text, None);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].player, "Steve");
        assert_eq!(events[0].action, "join");
        assert_eq!(events[1].player, "Alex");
        assert_eq!(events[1].observed_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(events[1].ip_kind, "loopback");
        assert_eq!(events[2].action, "leave");
    }

    #[test]
    fn rolls_event_date_across_midnight() {
        let text = "\
[23:59:50] [Server thread/INFO]: A joined the game\n\
[00:00:10] [Server thread/INFO]: B joined the game\n";
        let date = NaiveDate::from_ymd_opt(2026, 11, 12);
        let events = parse_player_events(text, date);
        assert_eq!(events.len(), 2);
        let first = DateTime::parse_from_rfc3339(events[0].at.as_deref().unwrap()).unwrap();
        let second = DateTime::parse_from_rfc3339(events[1].at.as_deref().unwrap()).unwrap();
        assert_eq!((second - first).num_seconds(), 20);
    }

    #[test]
    fn parses_srl_wrapped_lines_and_keeps_timestamps() {
        let text = "\
[stdout] [12:00:00] [Server thread/INFO]: Steve joined the game\n\
[stderr] [12:00:05] [Server thread/INFO]: Alex left the game\n";
        let date = NaiveDate::from_ymd_opt(2026, 11, 12);
        let events = parse_player_events(text, date);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].player, "Steve");
        let at = events[0].at.as_deref().expect("timestamp parsed");
        assert!(at.starts_with("2026-11-12T"), "{at}");
        assert_ne!(events[0].line, events[1].line);
    }

    #[test]
    fn validates_raknet_pong_strictly() {
        let mut packet = vec![0x1c];
        packet.extend_from_slice(&1i64.to_be_bytes());
        packet.extend_from_slice(&2i64.to_be_bytes());
        packet.extend_from_slice(&RAKNET_MAGIC);
        packet.extend_from_slice(&5u16.to_be_bytes());
        packet.extend_from_slice(b"MCPE;");
        assert!(valid_raknet_pong(&packet));
        assert!(!valid_raknet_pong(&packet[..10]));
        assert!(!valid_raknet_pong(&[]));
        let mut bad_magic = packet.clone();
        bad_magic[17] = 0xff;
        assert!(!valid_raknet_pong(&bad_magic));
        let mut bad_len = packet.clone();
        bad_len[33] = 0xff;
        bad_len[34] = 0xff;
        assert!(!valid_raknet_pong(&bad_len));
        let mut bad_utf8 = packet.clone();
        bad_utf8[35] = 0xff;
        assert!(!valid_raknet_pong(&bad_utf8));
        let mut bad_id = packet;
        bad_id[0] = 0x1d;
        assert!(!valid_raknet_pong(&bad_id));
    }

    #[test]
    fn classifies_private_and_public_ips() {
        assert_eq!(classify_ip("127.0.0.1"), "loopback");
        assert_eq!(classify_ip("192.168.1.10"), "lan");
        assert_eq!(classify_ip("10.0.0.2"), "lan");
        assert_eq!(classify_ip("8.8.8.8"), "public");
        assert_eq!(classify_ip("::1"), "loopback");
        assert_eq!(classify_ip("nonsense"), "unknown");
    }

    #[test]
    fn parses_netstat_rows_for_pid() {
        let output = "  TCP    127.0.0.1:25565      0.0.0.0:0       LISTENING       4242\n\
                        UDP    0.0.0.0:19132          *:*                             4242\n";
        assert_eq!(parse_netstat_ports(output, 4242), vec![19132, 25565]);
    }
}
