use crate::network_protocol::PeerAddr;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Time to persist Accounts Id in the router without removing them in seconds.
pub const TTL_ACCOUNT_ID_ROUTER: u64 = 60 * 60;

/// Maximum number of active peers. Hard limit.
fn default_max_num_peers() -> u32 {
    40
}
/// Minimum outbound connections a peer should have to avoid eclipse attacks.
fn default_minimum_outbound_connections() -> u32 {
    5
}
/// Lower bound of the ideal number of connections.
fn default_ideal_connections_lo() -> u32 {
    30
}
/// Upper bound of the ideal number of connections.
fn default_ideal_connections_hi() -> u32 {
    35
}
/// Peers which last message is was within this period of time are considered active recent peers.
fn default_peer_recent_time_window() -> Duration {
    Duration::from_secs(600)
}
/// Number of peers to keep while removing a connection.
/// Used to avoid disconnecting from peers we have been connected since long time.
fn default_safe_set_size() -> u32 {
    20
}
/// Lower bound of the number of connections to archival peers to keep
/// if we are an archival node.
fn default_archival_peer_connections_lower_bound() -> u32 {
    10
}
/// Time to persist Accounts Id in the router without removing them in seconds.
fn default_ttl_account_id_router() -> Duration {
    Duration::from_secs(TTL_ACCOUNT_ID_ROUTER)
}
/// Period to check on peer status
fn default_peer_stats_period() -> Duration {
    Duration::from_secs(5)
}
/// Period to update the list of peers we connect to.
fn default_monitor_peers_max_period() -> Duration {
    Duration::from_secs(60)
}

/// Remove peers that we didn't hear about for this amount of time.
fn default_peer_expiration_duration() -> Duration {
    Duration::from_secs(7 * 24 * 60 * 60)
}

// If non-zero - we'll skip sending tombstones during initial sync and for that many seconds after start.
fn default_skip_tombstones() -> i64 {
    // Enable by default in shardnet only.
    if cfg!(feature = "shardnet") {
        // Skip sending tombstones during sync and 240 seconds after start.
        240
    } else {
        0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    /// Local address to listen for incoming connections.
    pub addr: String,
    /// Comma separated list of nodes to connect to.
    /// Examples:
    ///   ed25519:86EtEy7epneKyrcJwSWP7zsisTkfDRH5CFVszt4qiQYw@31.192.22.209:24567
    ///   ed25519:86EtEy7epneKyrcJwSWP7zsisTkfDRH5CFVszt4qiQYw@nearnode.com:24567
    pub boot_nodes: String,
    /// Comma separated list of whitelisted nodes. Inbound connections from the nodes on
    /// the whitelist are accepted even if the limit of the inbound connection has been reached.
    /// For each whitelisted node specifying both PeerId and one of IP:port or Host:port is required:
    /// Examples:
    ///   ed25519:86EtEy7epneKyrcJwSWP7zsisTkfDRH5CFVszt4qiQYw@31.192.22.209:24567
    ///   ed25519:86EtEy7epneKyrcJwSWP7zsisTkfDRH5CFVszt4qiQYw@nearnode.com:24567
    #[serde(default)]
    pub whitelist_nodes: String,
    /// Maximum number of active peers. Hard limit.
    #[serde(default = "default_max_num_peers")]
    pub max_num_peers: u32,
    /// Minimum outbound connections a peer should have to avoid eclipse attacks.
    #[serde(default = "default_minimum_outbound_connections")]
    pub minimum_outbound_peers: u32,
    /// Lower bound of the ideal number of connections.
    #[serde(default = "default_ideal_connections_lo")]
    pub ideal_connections_lo: u32,
    /// Upper bound of the ideal number of connections.
    #[serde(default = "default_ideal_connections_hi")]
    pub ideal_connections_hi: u32,
    /// Peers which last message is was within this period of time are considered active recent peers (in seconds).
    #[serde(default = "default_peer_recent_time_window")]
    pub peer_recent_time_window: Duration,
    /// Number of peers to keep while removing a connection.
    /// Used to avoid disconnecting from peers we have been connected since long time.
    #[serde(default = "default_safe_set_size")]
    pub safe_set_size: u32,
    /// Lower bound of the number of connections to archival peers to keep
    /// if we are an archival node.
    #[serde(default = "default_archival_peer_connections_lower_bound")]
    pub archival_peer_connections_lower_bound: u32,
    /// Handshake timeout.
    pub handshake_timeout: Duration,
    /// Skip waiting for peers before starting node.
    pub skip_sync_wait: bool,
    /// Ban window for peers who misbehave.
    pub ban_window: Duration,
    /// List of addresses that will not be accepted as valid neighbors.
    /// It can be IP:Port or IP (to blacklist all connections coming from this address).
    #[serde(default)]
    pub blacklist: Vec<String>,
    /// Time to persist Accounts Id in the router without removing them in seconds.
    #[serde(default = "default_ttl_account_id_router")]
    pub ttl_account_id_router: Duration,
    /// Period to check on peer status
    #[serde(default = "default_peer_stats_period")]
    pub peer_stats_period: Duration,
    // Period to monitor peers (connect to new ones etc).
    #[serde(default = "default_monitor_peers_max_period")]
    pub monitor_peers_max_period: Duration,

    // Remove peers that were not active for this amount of time.
    #[serde(default = "default_peer_expiration_duration")]
    pub peer_expiration_duration: Duration,

    /// List of the public addresses (in the format "<node public key>@<IP>:<port>") of trusted nodes,
    /// which are willing to route messages to this node. Useful only if this node is a validator.
    /// This list will be signed and broadcasted to the whole network, so that everyone
    /// knows how to reach the validator.
    ///
    /// Example:
    ///   ["ed25519:86EtEy7epneKyrcJwSWP7zsisTkfDRH5CFVszt4qiQYw@31.192.22.209:24567"]
    ///
    /// Recommended setup (requires public static IP):
    /// In the simplest case this list should contains just 1 public address (with the node public
    /// key) of this validator.
    /// In case the validator doesn't have a public IP (i.e. it is hidden in a private network),
    /// this list should contain public addresses of the trusted nodes which will be routing messages to the
    /// validator - validator will connect to these nodes immediately after startup.
    /// TODO(gprusak): in case a connection cannot be established (the peer is
    /// unreachable/down/etc.) validator should probably remove (temporarily) the problematic peer from the list
    /// and broadcast the new version of the list.
    ///
    /// Less recommended setup (requires exactly one public dynamic/ephemeral or static IP):
    /// If the list is empty, the validator node will query trusted_stun_servers to determine its own IP.
    /// Only if the answer from the STUN servers is unambiguous (at least 1 server responds and
    /// all received responses provide the same IP), the IP (together with the port deduced from
    /// the addr field in this config) will be signed and broadcasted.
    ///
    /// Discouraged setup (might be removed in the future)
    /// If the list is empty and STUN servers' response is ambiguous, the peers which connect to
    /// this validator node will natually observe the address of the validator and broadcast it.
    /// This setup is not reliable in presence of byzantine peers.
    #[serde(default)]
    pub public_addrs: Vec<PeerAddr>,
    /// List of endpoints of trusted [STUN servers](https://datatracker.ietf.org/doc/html/rfc8489).
    ///
    /// Used only if this node is a validator and public_ips is empty (see
    /// description of public_ips field).  Format `<domain/ip>:<port>`, for
    /// example `stun.l.google.com:19302`.
    // TODO: unskip, once the functionality is implemented.
    #[serde(skip)] // TODO: add a default list.
    pub trusted_stun_servers: Vec<String>,
    // Experimental part of the JSON config. Regular users/validators should not have to set any values there.
    // Field names in here can change/disappear at any moment without warning.
    #[serde(default)]
    pub experimental: ExperimentalConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExperimentalConfig {
    // If true - don't allow any inbound connections.
    #[serde(default)]
    pub inbound_disabled: bool,
    // If true - connect only to the boot nodes.
    #[serde(default)]
    pub connect_only_to_boot_nodes: bool,

    // If greater than 0, then system will no longer send or receive tombstones
    // during sync and during that many seconds after startup.
    //
    // The better name is `skip_tombstones_seconds`, but we keep send for
    // compatibility.
    #[serde(default = "default_skip_tombstones")]
    pub skip_sending_tombstones_seconds: i64,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        ExperimentalConfig {
            inbound_disabled: false,
            connect_only_to_boot_nodes: false,
            skip_sending_tombstones_seconds: default_skip_tombstones(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            addr: "0.0.0.0:24567".to_string(),
            boot_nodes: "".to_string(),
            whitelist_nodes: "".to_string(),
            max_num_peers: default_max_num_peers(),
            minimum_outbound_peers: default_minimum_outbound_connections(),
            ideal_connections_lo: default_ideal_connections_lo(),
            ideal_connections_hi: default_ideal_connections_hi(),
            peer_recent_time_window: default_peer_recent_time_window(),
            safe_set_size: default_safe_set_size(),
            archival_peer_connections_lower_bound: default_archival_peer_connections_lower_bound(),
            handshake_timeout: Duration::from_secs(20),
            skip_sync_wait: false,
            ban_window: Duration::from_secs(3 * 60 * 60),
            blacklist: vec![],
            ttl_account_id_router: default_ttl_account_id_router(),
            peer_stats_period: default_peer_stats_period(),
            monitor_peers_max_period: default_monitor_peers_max_period(),
            peer_expiration_duration: default_peer_expiration_duration(),
            public_addrs: vec![],
            trusted_stun_servers: vec![],
            experimental: Default::default(),
        }
    }
}
