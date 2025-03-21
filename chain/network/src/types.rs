/// Type that belong to the network protocol.
pub use crate::network_protocol::{
    AccountOrPeerIdOrHash, Encoding, Handshake, HandshakeFailureReason, PeerMessage,
    RoutingTableUpdate, SignedAccountData,
};
use crate::routing::routing_table_view::RoutingTableInfo;
use crate::time;
use futures::future::BoxFuture;
use futures::FutureExt;
use near_crypto::PublicKey;
use near_o11y::WithSpanContext;
use near_primitives::block::{ApprovalMessage, Block};
use near_primitives::challenge::Challenge;
use near_primitives::hash::CryptoHash;
use near_primitives::network::{AnnounceAccount, PeerId};
use near_primitives::sharding::PartialEncodedChunkWithArcReceipts;
use near_primitives::transaction::SignedTransaction;
use near_primitives::types::BlockHeight;
use near_primitives::types::{AccountId, EpochId, ShardId};
use near_primitives::views::{KnownProducerView, NetworkInfoView, PeerInfoView};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;

/// Exported types, which are part of network protocol.
pub use crate::network_protocol::{
    Edge, PartialEdgeInfo, PartialEncodedChunkForwardMsg, PartialEncodedChunkRequestMsg,
    PartialEncodedChunkResponseMsg, PeerChainInfo, PeerChainInfoV2, PeerIdOrHash, PeerInfo, Ping,
    Pong, StateResponseInfo, StateResponseInfoV1, StateResponseInfoV2,
};

/// Number of hops a message is allowed to travel before being dropped.
/// This is used to avoid infinite loop because of inconsistent view of the network
/// by different nodes.
pub const ROUTED_MESSAGE_TTL: u8 = 100;

/// Peer type.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, strum::IntoStaticStr)]
pub enum PeerType {
    /// Inbound session
    Inbound,
    /// Outbound session
    Outbound,
}

#[derive(Debug, Clone)]
pub struct KnownProducer {
    pub account_id: AccountId,
    pub addr: Option<SocketAddr>,
    pub peer_id: PeerId,
    pub next_hops: Option<Vec<PeerId>>,
}

/// Ban reason.
#[derive(borsh::BorshSerialize, borsh::BorshDeserialize, Debug, Clone, PartialEq, Eq, Copy)]
pub enum ReasonForBan {
    None = 0,
    BadBlock = 1,
    BadBlockHeader = 2,
    HeightFraud = 3,
    BadHandshake = 4,
    BadBlockApproval = 5,
    Abusive = 6,
    InvalidSignature = 7,
    InvalidPeerId = 8,
    InvalidHash = 9,
    InvalidEdge = 10,
    Blacklisted = 14,
}

/// Banning signal sent from Peer instance to PeerManager
/// just before Peer instance is stopped.
#[derive(actix::Message, Debug)]
#[rtype(result = "()")]
pub struct Ban {
    pub peer_id: PeerId,
    pub ban_reason: ReasonForBan,
}

/// Status of the known peers.
#[derive(Eq, PartialEq, Debug, Clone)]
pub enum KnownPeerStatus {
    /// We got information about this peer from someone, but we didn't
    /// verify them yet. This peer might not exist, invalid IP etc.
    /// Also the peers that we failed to connect to, will be marked as 'Unknown'.
    Unknown,
    /// We know that this peer exists - we were connected to it, or it was provided as boot node.
    NotConnected,
    /// We're currently connected to this peer.
    Connected,
    /// We banned this peer for some reason. Once the ban time is over, it will move to 'NotConnected' state.
    Banned(ReasonForBan, time::Utc),
}

/// Information node stores about known peers.
#[derive(Debug, Clone)]
pub struct KnownPeerState {
    pub peer_info: PeerInfo,
    pub status: KnownPeerStatus,
    pub first_seen: time::Utc,
    pub last_seen: time::Utc,
    // Last time we tried to connect to this peer.
    // This data is not persisted in storage.
    pub last_outbound_attempt: Option<(time::Utc, Result<(), String>)>,
}

impl KnownPeerState {
    pub fn new(peer_info: PeerInfo, now: time::Utc) -> Self {
        KnownPeerState {
            peer_info,
            status: KnownPeerStatus::Unknown,
            first_seen: now,
            last_seen: now,
            last_outbound_attempt: None,
        }
    }
}

impl KnownPeerStatus {
    pub fn is_banned(&self) -> bool {
        matches!(self, KnownPeerStatus::Banned(_, _))
    }
}

/// Set of account keys.
/// This is information which chain pushes to network to implement tier1.
/// See ChainInfo.
pub type AccountKeys = HashMap<(EpochId, AccountId), PublicKey>;

/// Network-relevant data about the chain.
// TODO(gprusak): it is more like node info, or sth.
#[derive(Debug, Clone, Default)]
pub struct ChainInfo {
    pub tracked_shards: Vec<ShardId>,
    pub height: BlockHeight,
    // Public keys of accounts participating in the BFT consensus
    // (both accounts from current and next epoch are important, that's why
    // the map is indexed by (EpochId,AccountId) pair).
    // It currently includes "block producers", "chunk producers" and "approvers".
    // They are collectively known as "validators".
    // Peers acting on behalf of these accounts have a higher
    // priority on the NEAR network than other peers.
    pub tier1_accounts: Arc<AccountKeys>,
}

#[derive(Debug, actix::Message)]
#[rtype(result = "()")]
pub struct SetChainInfo(pub ChainInfo);

#[derive(Debug, actix::Message)]
#[rtype(result = "NetworkInfo")]
pub struct GetNetworkInfo;

/// Public actix interface of `PeerManagerActor`.
#[derive(actix::Message, Debug, strum::IntoStaticStr)]
#[rtype(result = "PeerManagerMessageResponse")]
pub enum PeerManagerMessageRequest {
    NetworkRequests(NetworkRequests),
    /// Request PeerManager to connect to the given peer.
    /// Used in tests and internally by PeerManager.
    /// TODO: replace it with AsyncContext::spawn/run_later for internal use.
    OutboundTcpConnect(crate::tcp::Stream),
    /// TEST-ONLY
    SetAdvOptions(crate::test_utils::SetAdvOptions),
    /// The following types of requests are used to trigger actions in the Peer Manager for testing.
    /// TEST-ONLY: Fetch current routing table.
    FetchRoutingTable,
    /// TEST-ONLY Start ping to `PeerId` with `nonce`.
    PingTo {
        nonce: u64,
        target: PeerId,
    },
}

impl PeerManagerMessageRequest {
    pub fn as_network_requests(self) -> NetworkRequests {
        if let PeerManagerMessageRequest::NetworkRequests(item) = self {
            item
        } else {
            panic!("expected PeerMessageRequest::NetworkRequests(");
        }
    }

    pub fn as_network_requests_ref(&self) -> &NetworkRequests {
        if let PeerManagerMessageRequest::NetworkRequests(item) = self {
            item
        } else {
            panic!("expected PeerMessageRequest::NetworkRequests");
        }
    }
}

/// List of all replies to messages to `PeerManager`. See `PeerManagerMessageRequest` for more details.
#[derive(actix::MessageResponse, Debug)]
pub enum PeerManagerMessageResponse {
    NetworkResponses(NetworkResponses),
    /// TEST-ONLY
    OutboundTcpConnect,
    SetAdvOptions,
    FetchRoutingTable(RoutingTableInfo),
    PingTo,
}

impl PeerManagerMessageResponse {
    pub fn as_network_response(self) -> NetworkResponses {
        if let PeerManagerMessageResponse::NetworkResponses(item) = self {
            item
        } else {
            panic!("expected PeerMessageRequest::NetworkResponses(");
        }
    }
}

impl From<NetworkResponses> for PeerManagerMessageResponse {
    fn from(msg: NetworkResponses) -> Self {
        PeerManagerMessageResponse::NetworkResponses(msg)
    }
}

// TODO(#1313): Use Box
#[derive(Clone, strum::AsRefStr, Debug, Eq, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum NetworkRequests {
    /// Sends block, either when block was just produced or when requested.
    Block { block: Block },
    /// Sends approval.
    Approval { approval_message: ApprovalMessage },
    /// Request block with given hash from given peer.
    BlockRequest { hash: CryptoHash, peer_id: PeerId },
    /// Request given block headers.
    BlockHeadersRequest { hashes: Vec<CryptoHash>, peer_id: PeerId },
    /// Request state header for given shard at given state root.
    StateRequestHeader { shard_id: ShardId, sync_hash: CryptoHash, target: AccountOrPeerIdOrHash },
    /// Request state part for given shard at given state root.
    StateRequestPart {
        shard_id: ShardId,
        sync_hash: CryptoHash,
        part_id: u64,
        target: AccountOrPeerIdOrHash,
    },
    /// Response to state request.
    StateResponse { route_back: CryptoHash, response: StateResponseInfo },
    /// Ban given peer.
    BanPeer { peer_id: PeerId, ban_reason: ReasonForBan },
    /// Announce account
    AnnounceAccount(AnnounceAccount),

    /// Request chunk parts and/or receipts
    PartialEncodedChunkRequest {
        target: AccountIdOrPeerTrackingShard,
        request: PartialEncodedChunkRequestMsg,
        create_time: time::Instant,
    },
    /// Information about chunk such as its header, some subset of parts and/or incoming receipts
    PartialEncodedChunkResponse { route_back: CryptoHash, response: PartialEncodedChunkResponseMsg },
    /// Information about chunk such as its header, some subset of parts and/or incoming receipts
    PartialEncodedChunkMessage {
        account_id: AccountId,
        partial_encoded_chunk: PartialEncodedChunkWithArcReceipts,
    },
    /// Forwarding a chunk part to a validator tracking the shard
    PartialEncodedChunkForward { account_id: AccountId, forward: PartialEncodedChunkForwardMsg },

    /// Valid transaction but since we are not validators we send this transaction to current validators.
    ForwardTx(AccountId, SignedTransaction),
    /// Query transaction status
    TxStatus(AccountId, AccountId, CryptoHash),
    /// A challenge to invalidate a block.
    Challenge(Challenge),
}

/// Combines peer address info, chain and edge information.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FullPeerInfo {
    pub peer_info: PeerInfo,
    pub chain_info: PeerChainInfoV2,
    pub partial_edge_info: PartialEdgeInfo,
}

impl From<&FullPeerInfo> for ConnectedPeerInfo {
    fn from(full_peer_info: &FullPeerInfo) -> Self {
        ConnectedPeerInfo {
            full_peer_info: full_peer_info.clone(),
            received_bytes_per_sec: 0,
            sent_bytes_per_sec: 0,
            last_time_peer_requested: time::Instant::now(),
            last_time_received_message: time::Instant::now(),
            connection_established_time: time::Instant::now(),
            peer_type: PeerType::Outbound,
        }
    }
}

impl From<&ConnectedPeerInfo> for PeerInfoView {
    fn from(connected_peer_info: &ConnectedPeerInfo) -> Self {
        let full_peer_info = &connected_peer_info.full_peer_info;
        PeerInfoView {
            addr: match full_peer_info.peer_info.addr {
                Some(socket_addr) => socket_addr.to_string(),
                None => "N/A".to_string(),
            },
            account_id: full_peer_info.peer_info.account_id.clone(),
            height: full_peer_info.chain_info.height,
            tracked_shards: full_peer_info.chain_info.tracked_shards.clone(),
            archival: full_peer_info.chain_info.archival,
            peer_id: full_peer_info.peer_info.id.public_key().clone(),
            received_bytes_per_sec: connected_peer_info.received_bytes_per_sec,
            sent_bytes_per_sec: connected_peer_info.sent_bytes_per_sec,
            last_time_peer_requested_millis: connected_peer_info
                .last_time_peer_requested
                .elapsed()
                .whole_milliseconds() as u64,
            last_time_received_message_millis: connected_peer_info
                .last_time_received_message
                .elapsed()
                .whole_milliseconds() as u64,
            connection_established_time_millis: connected_peer_info
                .connection_established_time
                .elapsed()
                .whole_milliseconds() as u64,
            is_outbound_peer: connected_peer_info.peer_type == PeerType::Outbound,
        }
    }
}

// Information about the connected peer that is shared with the rest of the system.
#[derive(Debug, Clone)]
pub struct ConnectedPeerInfo {
    pub full_peer_info: FullPeerInfo,
    /// Number of bytes we've received from the peer.
    pub received_bytes_per_sec: u64,
    /// Number of bytes we've sent to the peer.
    pub sent_bytes_per_sec: u64,
    /// Last time requested peers.
    pub last_time_peer_requested: time::Instant,
    /// Last time we received a message from this peer.
    pub last_time_received_message: time::Instant,
    /// Time where the connection was established.
    pub connection_established_time: time::Instant,
    /// Who started connection. Inbound (other) or Outbound (us).
    pub peer_type: PeerType,
}

#[derive(Debug, Clone, actix::MessageResponse)]
pub struct NetworkInfo {
    pub connected_peers: Vec<ConnectedPeerInfo>,
    pub num_connected_peers: usize,
    pub peer_max_count: u32,
    pub highest_height_peers: Vec<FullPeerInfo>,
    pub sent_bytes_per_sec: u64,
    pub received_bytes_per_sec: u64,
    /// Accounts of known block and chunk producers from routing table.
    pub known_producers: Vec<KnownProducer>,
    pub tier1_accounts: Vec<Arc<SignedAccountData>>,
}

impl From<NetworkInfo> for NetworkInfoView {
    fn from(network_info: NetworkInfo) -> Self {
        NetworkInfoView {
            peer_max_count: network_info.peer_max_count,
            num_connected_peers: network_info.num_connected_peers,
            connected_peers: network_info
                .connected_peers
                .iter()
                .map(|full_peer_info| full_peer_info.into())
                .collect::<Vec<_>>(),
            known_producers: network_info
                .known_producers
                .iter()
                .map(|it| KnownProducerView {
                    account_id: it.account_id.clone(),
                    peer_id: it.peer_id.public_key().clone(),
                    next_hops: it
                        .next_hops
                        .as_ref()
                        .map(|it| it.iter().map(|peer_id| peer_id.public_key().clone()).collect()),
                })
                .collect(),
        }
    }
}

#[derive(Debug, actix::MessageResponse)]
pub enum NetworkResponses {
    NoResponse,
    PingPongInfo { pings: Vec<Ping>, pongs: Vec<Pong> },
    RouteNotFound,
}

#[cfg(feature = "test_features")]
#[derive(actix::Message, Debug)]
#[rtype(result = "Option<u64>")]
pub enum NetworkAdversarialMessage {
    AdvProduceBlocks(u64, bool),
    AdvSwitchToHeight(u64),
    AdvDisableHeaderSync,
    AdvDisableDoomslug,
    AdvGetSavedBlocks,
    AdvCheckStorageConsistency,
    AdvSetSyncInfo(u64),
}

pub trait MsgRecipient<M: actix::Message>: Send + Sync + 'static {
    fn send(&self, msg: M) -> BoxFuture<'static, Result<M::Result, actix::MailboxError>>;
    fn do_send(&self, msg: M);
}

impl<A, M> MsgRecipient<M> for actix::Addr<A>
where
    M: actix::Message + Send + 'static,
    M::Result: Send,
    A: actix::Actor + actix::Handler<M>,
    A::Context: actix::dev::ToEnvelope<A, M>,
{
    fn send(&self, msg: M) -> BoxFuture<'static, Result<M::Result, actix::MailboxError>> {
        actix::Addr::send(self, msg).boxed()
    }
    fn do_send(&self, msg: M) {
        actix::Addr::do_send(self, msg)
    }
}
pub trait PeerManagerAdapter:
    MsgRecipient<WithSpanContext<PeerManagerMessageRequest>>
    + MsgRecipient<WithSpanContext<SetChainInfo>>
{
}
impl<
        A: MsgRecipient<WithSpanContext<PeerManagerMessageRequest>>
            + MsgRecipient<WithSpanContext<SetChainInfo>>,
    > PeerManagerAdapter for A
{
}

pub struct NetworkRecipient<T> {
    recipient: OnceCell<Arc<T>>,
}

impl<T> Default for NetworkRecipient<T> {
    fn default() -> Self {
        Self { recipient: OnceCell::default() }
    }
}

impl<T> NetworkRecipient<T> {
    pub fn set_recipient(&self, t: T) {
        self.recipient.set(Arc::new(t)).ok().expect("cannot set recipient twice");
    }
}

impl<M: actix::Message, T: MsgRecipient<M>> MsgRecipient<M> for NetworkRecipient<T> {
    fn send(&self, msg: M) -> BoxFuture<'static, Result<M::Result, actix::MailboxError>> {
        self.recipient.wait().send(msg)
    }
    fn do_send(&self, msg: M) {
        self.recipient.wait().do_send(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network_protocol::{RawRoutedMessage, RoutedMessage, RoutedMessageBody};
    use borsh::BorshSerialize as _;
    use near_primitives::syncing::ShardStateSyncResponseV1;

    const ALLOWED_SIZE: usize = 1 << 20;
    const NOTIFY_SIZE: usize = 1024;

    macro_rules! assert_size {
        ($type:ident) => {
            let struct_size = std::mem::size_of::<$type>();
            if struct_size >= NOTIFY_SIZE {
                println!("The size of {} is {}", stringify!($type), struct_size);
            }
            assert!(struct_size <= ALLOWED_SIZE);
        };
    }

    #[test]
    fn test_size() {
        assert_size!(HandshakeFailureReason);
        assert_size!(NetworkRequests);
        assert_size!(NetworkResponses);
        assert_size!(Handshake);
        assert_size!(Ping);
        assert_size!(Pong);
        assert_size!(RoutingTableUpdate);
        assert_size!(FullPeerInfo);
        assert_size!(NetworkInfo);
    }

    macro_rules! assert_size {
        ($type:ident) => {
            let struct_size = std::mem::size_of::<$type>();
            if struct_size >= NOTIFY_SIZE {
                println!("The size of {} is {}", stringify!($type), struct_size);
            }
            assert!(struct_size <= ALLOWED_SIZE);
        };
    }

    #[test]
    fn test_enum_size() {
        assert_size!(PeerType);
        assert_size!(RoutedMessageBody);
        assert_size!(PeerIdOrHash);
        assert_size!(KnownPeerStatus);
        assert_size!(ReasonForBan);
    }

    #[test]
    fn test_struct_size() {
        assert_size!(PeerInfo);
        assert_size!(AnnounceAccount);
        assert_size!(Ping);
        assert_size!(Pong);
        assert_size!(RawRoutedMessage);
        assert_size!(RoutedMessage);
        assert_size!(KnownPeerState);
        assert_size!(Ban);
        assert_size!(StateResponseInfoV1);
        assert_size!(PartialEncodedChunkRequestMsg);
    }

    #[test]
    fn routed_message_body_compatibility_smoke_test() {
        #[track_caller]
        fn check(msg: RoutedMessageBody, expected: &[u8]) {
            let actual = msg.try_to_vec().unwrap();
            assert_eq!(actual.as_slice(), expected);
        }

        check(
            RoutedMessageBody::TxStatusRequest("test_x".parse().unwrap(), CryptoHash([42; 32])),
            &[
                2, 6, 0, 0, 0, 116, 101, 115, 116, 95, 120, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
                42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 42,
                42,
            ],
        );

        check(
            RoutedMessageBody::VersionedStateResponse(StateResponseInfo::V1(StateResponseInfoV1 {
                shard_id: 62,
                sync_hash: CryptoHash([92; 32]),
                state_response: ShardStateSyncResponseV1 { header: None, part: None },
            })),
            &[
                17, 0, 62, 0, 0, 0, 0, 0, 0, 0, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92,
                92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 92, 0, 0,
            ],
        );
    }
}

// Don't need Borsh ?
#[derive(Debug, Clone, PartialEq, Eq, borsh::BorshSerialize, borsh::BorshDeserialize, Hash)]
/// Defines the destination for a network request.
/// The request should be sent either to the `account_id` as a routed message, or directly to
/// any peer that tracks the shard.
/// If `prefer_peer` is `true`, should be sent to the peer, unless no peer tracks the shard, in which
/// case fall back to sending to the account.
/// Otherwise, send to the account, unless we do not know the route, in which case send to the peer.
pub struct AccountIdOrPeerTrackingShard {
    /// Target account to send the the request to
    pub account_id: Option<AccountId>,
    /// Whether to check peers first or target account first
    pub prefer_peer: bool,
    /// Select peers that track shard `shard_id`
    pub shard_id: ShardId,
    /// Select peers that are archival nodes if it is true
    pub only_archival: bool,
    /// Only send messages to peers whose latest chain height is no less `min_height`
    pub min_height: BlockHeight,
}
