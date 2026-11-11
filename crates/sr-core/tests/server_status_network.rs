//! Real Minecraft Server List Ping. Ignored by default because it requires
//! network access and a reachable public server.

use sr_core::multiplayer::server_status::ping_server;

#[tokio::test]
#[ignore = "fetches a real public Minecraft server"]
async fn pings_real_public_server() {
    let report = ping_server("mc.hypixel.net:25565").await;
    println!(
        "ok={} address={} version={:?} protocol={:?} players={:?}/{:?} latency={:?} error={:?}",
        report.ok,
        report.address,
        report.version_name,
        report.protocol,
        report.online_players,
        report.max_players,
        report.latency_ms,
        report.error
    );
    assert!(report.ok, "report: {:?}", report.error);
    assert!(report.version_name.is_some());
    assert!(report.protocol.is_some());
    assert!(report.max_players.unwrap_or(0) > 0);
}
