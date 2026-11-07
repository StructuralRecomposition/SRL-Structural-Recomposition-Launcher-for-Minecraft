use std::time::{Duration, Instant};
use tokio::net::TcpStream;

use super::provider::{NodeStat, ProviderNode};
use super::types::{Edition, NodeView};

pub async fn tcp_latency(host: &str, port: u16) -> Option<u64> {
    if host.trim().is_empty() || port == 0 {
        return None;
    }
    let started = Instant::now();
    let result =
        tokio::time::timeout(Duration::from_secs(2), TcpStream::connect((host, port))).await;
    match result {
        Ok(Ok(_)) => Some(started.elapsed().as_millis() as u64),
        _ => None,
    }
}

pub fn rank_nodes(
    nodes: &[ProviderNode],
    stats: &[NodeStat],
    latencies: &[(u64, Option<u64>)],
    edition: Edition,
) -> Vec<NodeView> {
    let mut ranked: Vec<NodeView> = nodes
        .iter()
        .map(|node| {
            let stat = stats.iter().find(|s| s.node_id == node.id);
            let latency = latencies
                .iter()
                .find(|(id, _)| *id == node.id)
                .and_then(|(_, value)| *value);
            let allowed = match edition {
                Edition::Java => true,
                Edition::Bedrock => node.allow_udp,
            };
            let reason = if node.host.trim().is_empty() {
                Some("节点未返回可连接主机名".to_string())
            } else if !allowed {
                Some("节点不支持 UDP".to_string())
            } else {
                None
            };
            let score = if allowed && reason.is_none() {
                score_node(node, stat, latency)
            } else {
                0.0
            };
            NodeView {
                id: node.id,
                name: node.name.clone(),
                host: node.host.clone(),
                server_port: node.server_port,
                description: node.description.clone(),
                allow_udp: node.allow_udp,
                allow_big_traffic: node.allow_big_traffic,
                need_icp: node.need_icp,
                latency_ms: latency,
                clients: stat.and_then(|s| s.clients),
                outbound: stat.and_then(|s| s.outbound),
                score,
                available: allowed && reason.is_none(),
                reason,
            }
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.available
            .cmp(&a.available)
            .then_with(|| b.score.total_cmp(&a.score))
    });
    ranked
}

fn score_node(node: &ProviderNode, stat: Option<&NodeStat>, latency: Option<u64>) -> f64 {
    let latency_score = match latency {
        Some(ms) => 1.0 / (1.0 + ms as f64 / 120.0),
        None => 0.05,
    };
    let clients = stat.and_then(|s| s.clients).unwrap_or(0) as f64;
    let outbound_gb = stat
        .and_then(|s| s.outbound)
        .map(|v| {
            if v > 1024.0 * 1024.0 {
                v / 1024.0 / 1024.0 / 1024.0
            } else {
                v
            }
        })
        .unwrap_or(0.0);
    let load_score = 1.0 / (1.0 + clients / 24.0) / (1.0 + outbound_gb.min(500.0) / 80.0);
    let mut score = 0.58 * latency_score + 0.32 * load_score + 0.10;
    if node.allow_big_traffic {
        score += 0.06;
    }
    if node.need_icp {
        score -= 0.12;
    }
    score.max(0.0)
}

/// Validate that a provider-assigned port falls inside one of the node's
/// published ranges. Accepts both `20000-30000` and single ports.
pub fn port_allowed(ranges: &[String], port: u16) -> bool {
    if ranges.is_empty() {
        return true;
    }
    ranges.iter().any(|range| {
        let range = range.trim();
        if let Some((start, end)) = range.split_once('-') {
            let start = start.trim().parse::<u32>().ok();
            let end = end.trim().parse::<u32>().ok();
            matches!((start, end), (Some(a), Some(b)) if (a..=b).contains(&(port as u32)))
        } else {
            range.parse::<u16>().ok() == Some(port)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplayer::provider::ProviderNode;

    fn node(id: u64, udp: bool) -> ProviderNode {
        ProviderNode {
            id,
            name: format!("node-{id}"),
            host: "127.0.0.1".into(),
            ip: None,
            description: None,
            server_port: 7000,
            port_range: vec!["20000-30000".into()],
            allow_udp: udp,
            allow_http: true,
            allow_big_traffic: false,
            need_icp: false,
            verification_level: None,
        }
    }

    #[test]
    fn udp_filter_excludes_tcp_only_nodes() {
        let ranked = rank_nodes(&[node(1, false), node(2, true)], &[], &[], Edition::Bedrock);
        assert_eq!(ranked[0].id, 2);
        assert!(ranked[0].available);
        assert!(!ranked[1].available);
    }

    #[test]
    fn lower_latency_and_load_wins() {
        let nodes = [node(1, true), node(2, true)];
        let stats = [
            NodeStat {
                node_id: 1,
                clients: Some(80),
                outbound: Some(100.0),
                ..Default::default()
            },
            NodeStat {
                node_id: 2,
                clients: Some(2),
                outbound: Some(5.0),
                ..Default::default()
            },
        ];
        let ranked = rank_nodes(
            &nodes,
            &stats,
            &[(1, Some(180)), (2, Some(40))],
            Edition::Java,
        );
        assert_eq!(ranked[0].id, 2);
    }

    #[test]
    fn validates_port_ranges() {
        let ranges = vec!["20000-30000".to_string(), "40000".to_string()];
        assert!(port_allowed(&ranges, 25565));
        assert!(port_allowed(&ranges, 40000));
        assert!(!port_allowed(&ranges, 65500));
        assert!(port_allowed(&[], 1));
    }
}
