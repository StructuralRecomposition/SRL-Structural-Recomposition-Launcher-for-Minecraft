use crate::instance::Instance;
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
            // lsof returns no output rather than an error when nothing matches.
            return Vec::new();
        }

        let flag = if edition == Edition::Java {
            "-ltnp"
        } else {
            "-lunp"
        };
        if let Some(output) = run_command("ss", &["-H", flag]).await {
            return parse_ss_ports(&output, pid);
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
    fn parses_netstat_rows_for_pid() {
        let output = "  TCP    127.0.0.1:25565      0.0.0.0:0       LISTENING       4242\n\
                        UDP    0.0.0.0:19132          *:*                             4242\n";
        assert_eq!(parse_netstat_ports(output, 4242), vec![19132, 25565]);
    }
}
