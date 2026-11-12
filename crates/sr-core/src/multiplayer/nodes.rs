use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

use super::provider::ProviderNode;
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
    load: &HashMap<u64, f64>,
    latencies: &[(u64, Option<u64>)],
    edition: Edition,
) -> Vec<NodeView> {
    let mut ranked: Vec<NodeView> = nodes
        .iter()
        .map(|node| {
            let latency = latencies
                .iter()
                .find(|(id, _)| *id == node.id)
                .and_then(|(_, value)| *value);
            let load_value = load.get(&node.id).copied();
            let supports = node
                .tunnel_protocols
                .iter()
                .any(|p| p.eq_ignore_ascii_case(edition.tunnel_type()));
            let quota_ok = node.max_proxies < 0 || (node.proxy_count as i64) < node.max_proxies;
            let reason = if !node.available {
                Some("当前套餐未覆盖该节点".to_string())
            } else if !node.is_online {
                Some("节点当前离线".to_string())
            } else if !supports {
                Some(format!(
                    "节点不支持 {}",
                    edition.tunnel_type().to_uppercase()
                ))
            } else if !quota_ok {
                Some("节点隧道数量已达上限".to_string())
            } else if node.host.trim().is_empty() {
                Some("节点未返回主机名".to_string())
            } else {
                None
            };
            let available = reason.is_none();
            NodeView {
                id: node.id,
                name: node.name.clone(),
                host: node.host.clone(),
                region: [node.province.clone(), node.city.clone()]
                    .into_iter()
                    .flatten()
                    .filter(|v| !v.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
                description: node.description.clone(),
                allow_udp: node
                    .tunnel_protocols
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case("udp")),
                allow_big_traffic: false,
                need_icp: false,
                latency_ms: latency,
                clients: Some(node.proxy_count),
                load: load_value,
                load_available: load_value.is_some(),
                score: if available {
                    score_node(node, latency, load_value)
                } else {
                    0.0
                },
                available,
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

fn score_node(node: &ProviderNode, latency: Option<u64>, load: Option<f64>) -> f64 {
    let latency_score = match latency {
        Some(ms) => 1.0 / (1.0 + ms as f64 / 120.0),
        None => 0.05,
    };
    // Unknown load must not be treated as a healthy 30%. Use a neutral score
    // slightly below the midpoint so missing data cannot win by default.
    let load_score = load
        .map(|value| 1.0 / (1.0 + value.clamp(0.0, 100.0) / 35.0))
        .unwrap_or(0.45);
    let quota_score = if node.max_proxies < 0 {
        1.0
    } else {
        1.0 - (node.proxy_count as f64 / node.max_proxies.max(1) as f64).clamp(0.0, 1.0)
    };
    0.52 * latency_score + 0.33 * load_score + 0.15 * quota_score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, udp: bool) -> ProviderNode {
        ProviderNode {
            id,
            name: format!("node-{id}"),
            host: "127.0.0.1".into(),
            ip: None,
            city: Some("上海".into()),
            province: Some("上海".into()),
            description: None,
            available: true,
            is_online: true,
            proxy_count: 1,
            max_proxies: 10,
            tunnel_protocols: if udp {
                vec!["tcp".into(), "udp".into()]
            } else {
                vec!["tcp".into()]
            },
            proxies_allowed: "20000-30000".into(),
            proxies_denied: String::new(),
            tcp_port: 7000,
            load: None,
        }
    }

    #[test]
    fn udp_filter_excludes_tcp_only_nodes() {
        let ranked = rank_nodes(
            &[node(1, false), node(2, true)],
            &HashMap::new(),
            &[],
            Edition::Bedrock,
        );
        assert_eq!(ranked[0].id, 2);
        assert!(ranked[0].available);
        assert!(!ranked[1].available);
    }

    #[test]
    fn lower_latency_and_load_wins() {
        let nodes = [node(1, true), node(2, true)];
        let mut load = HashMap::new();
        load.insert(1, 85.0);
        load.insert(2, 10.0);
        let ranked = rank_nodes(
            &nodes,
            &load,
            &[(1, Some(180)), (2, Some(40))],
            Edition::Java,
        );
        assert_eq!(ranked[0].id, 2);
    }

    #[test]
    fn offlines_and_full_nodes_are_unavailable() {
        let mut offline = node(1, true);
        offline.is_online = false;
        let mut full = node(2, true);
        full.proxy_count = 10;
        let ranked = rank_nodes(&[offline, full], &HashMap::new(), &[], Edition::Java);
        assert!(ranked.iter().all(|n| !n.available));
    }
}
