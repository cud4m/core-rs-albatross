mod serialization;

use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::Path;
use std::str::FromStr;
use std::convert::TryFrom;

use failure::Error;
use log::LevelFilter;
use url::Url;
use hex::FromHex;

use network_primitives::{address, protocol};
use network_primitives::address::peer_uri::PeerUriError;
use network_primitives::networks::NetworkId;
use network::network_config::{ReverseProxyConfig, Seed as NetworkSeed};
use primitives::coin::Coin;
use keys::PublicKey;
use mempool::{MempoolConfig};
use mempool::filter::{Rules as MempoolRules, MempoolFilter};

use crate::config::config_file::serialization::*;
use crate::config::config;
use crate::config::consts;


#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default)]
    pub network: NetworkSettings,
    #[serde(default)]
    pub consensus: ConsensusSettings,
    pub rpc_server: Option<RpcServerSettings>,
    pub metrics_server: Option<MetricsServerSettings>,
    pub reverse_proxy: Option<ReverseProxySettings>,
    #[serde(default)]
    pub log: LogSettings,
    #[serde(default)]
    pub database: DatabaseSettings,
    pub mempool: Option<MempoolSettings>,
    #[serde(default)]
    pub peer_key_file: Option<String>,
    #[serde(default)]
    pub validator: Option<ValidatorSettings>,
}

impl ConfigFile {
    pub fn from_str<S: AsRef<str>>(config: S) -> Result<ConfigFile, Error> {
        Ok(toml::from_str(config.as_ref())?)
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<ConfigFile, Error> {
        Self::from_str(read_to_string(path)?)
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct NetworkSettings {
    pub host: Option<String>,
    pub port: Option<u16>,
    #[serde(default)]
    pub protocol: Protocol,
    #[serde(default)]
    pub seed_nodes: Vec<Seed>,
    #[serde(default)]
    pub user_agent: Option<String>,
    pub tls: Option<TlsSettings>,
    pub instant_inbound: Option<bool>,
}

#[derive(Debug, Fail)]
pub enum SeedError {
    #[fail(display = "Failed to parse peer URI: {}", _0)]
    PeerUri(#[cause] PeerUriError),
    #[fail(display = "Failed to parse seed list URL: {}", _0)]
    Url(#[cause] url::ParseError),
    #[fail(display = "Failed to parse public key: {}", _0)]
    PublicKey(#[cause] keys::ParseError),
}

impl From<PeerUriError> for SeedError {
    fn from(e: PeerUriError) -> Self {
        SeedError::PeerUri(e)
    }
}

impl From<url::ParseError> for SeedError {
    fn from(e: url::ParseError) -> Self {
        SeedError::Url(e)
    }
}

impl From<keys::ParseError> for SeedError {
    fn from(e: keys::ParseError) -> Self {
        SeedError::PublicKey(e)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Seed {
    Uri(SeedUri),
    Info(SeedInfo),
    List(SeedList),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedUri {
    pub uri: String
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedInfo {
    pub host: String,
    pub port: Option<u16>,
    pub public_key: Option<String>,
    pub peer_id: Option<String>
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedList {
    pub list: String,
    pub public_key: Option<String>
}

impl TryFrom<Seed> for NetworkSeed {
    type Error = SeedError;

    fn try_from(seed: Seed) -> Result<Self, Self::Error> {
        Ok(match seed {
            Seed::Uri(SeedUri { uri }) => {
                NetworkSeed::Peer(Box::new(address::PeerUri::from_str(&uri)?))
            },
            Seed::Info(SeedInfo { host, port, public_key, peer_id }) => {
                // TODO: Implement this without having to instantiate a PeerUri
                NetworkSeed::Peer(Box::new(address::PeerUri::new_wss(host, port, peer_id, public_key)))
            },
            Seed::List(SeedList { list, public_key }) => {
                NetworkSeed::List(Box::new(address::SeedList::new(Url::from_str(&list)?, public_key
                    .map(PublicKey::from_hex).transpose()?)))
            }
        })
    }
}

#[derive(Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Wss,
    Ws,
    Dumb,
    Rtc,
}

impl Default for Protocol {
    fn default() -> Self {
        Protocol::Ws
    }
}

impl From<Protocol> for protocol::Protocol {
    fn from(protocol: Protocol) -> Self {
        match protocol {
            Protocol::Dumb => Self::Dumb,
            Protocol::Ws => Self::Ws,
            Protocol::Wss => Self::Wss,
            Protocol::Rtc => Self::Rtc,
        }
    }
}


#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsSettings {
    pub identity_file: String,
    pub identity_password: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConsensusSettings {
    #[serde(rename = "type")]
    #[serde(default)]
    pub consensus_type: ConsensusType,
    #[serde(default)]
    pub network: Network,
}

#[derive(Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConsensusType {
    Full,
    MacroSync,
}

impl Default for ConsensusType {
    fn default() -> Self {
        ConsensusType::Full
    }
}

impl FromStr for ConsensusType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Ok(match s.to_lowercase().as_str() {
            "full" => Self::Full,
            "macro-sync" => Self::MacroSync,
            _ => return Err(())
        })
    }
}

impl From<ConsensusType> for config::ConsensusConfig {
    fn from(consensus_type: ConsensusType) -> Self {
        match consensus_type {
            ConsensusType::Full => Self::Full,
            ConsensusType::MacroSync => Self::MacroSync,
        }
    }
}

#[derive(Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    Main,
    Test,
    Dev,
    TestAlbatross,
    DevAlbatross,
}

impl Default for Network {
    fn default() -> Self {
        Network::Main
    }
}

impl FromStr for Network {
    type Err = ();

    fn from_str(s: &str) -> Result<Network, ()> {
        Ok(match s.to_lowercase().as_str() {
            "main" => Network::Main,
            "test" => Network::Test,
            "dev" => Network::Dev,
            "test-albatross" => Network::TestAlbatross,
            "dev-albatross" => Network::DevAlbatross,
            _ => return Err(())
        })
    }
}

impl From<Network> for NetworkId {
    fn from(network: Network) -> NetworkId {
        match network {
            Network::Main => NetworkId::Main,
            Network::Test => NetworkId::Test,
            Network::Dev => NetworkId::Dev,
            Network::TestAlbatross => NetworkId::TestAlbatross,
            Network::DevAlbatross => NetworkId::DevAlbatross,
        }
    }
}


#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RpcServerSettings {
    #[serde(deserialize_with = "deserialize_string_option")]
    #[serde(default)]
    pub bind: Option<address::NetAddress>,
    pub port: Option<u16>,
    #[serde(default)]
    pub corsdomain: Vec<String>,
    #[serde(default)]
    pub allowip: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct MetricsServerSettings {
    #[serde(deserialize_with = "deserialize_string_option")]
    #[serde(default)]
    pub bind: Option<address::NetAddress>,
    pub port: Option<u16>,
    pub password: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReverseProxySettings {
    pub port: Option<u16>,
    #[serde(deserialize_with = "deserialize_string")]
    pub address: address::NetAddress,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub with_tls_termination: bool,
}

impl From<ReverseProxySettings> for ReverseProxyConfig {
    fn from(proxy: ReverseProxySettings) -> Self {
        ReverseProxyConfig {
            port: proxy.port.unwrap_or(consts::REVERSE_PROXY_DEFAULT_PORT),
            address: proxy.address,
            header: proxy.header,
            with_tls_termination: proxy.with_tls_termination,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LogSettings {
    #[serde(deserialize_with = "deserialize_string_option")]
    #[serde(default)]
    pub level: Option<LevelFilter>,
    #[serde(default)]
    pub timestamps: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_tags")]
    pub tags: HashMap<String, LevelFilter>,
    #[serde(default)]
    pub statistics: u64,
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    pub path: Option<String>,
    pub size: Option<usize>,
    pub max_dbs: Option<u32>,
    pub no_lmdb_sync: Option<bool>,
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        DatabaseSettings {
            path: None,
            size: Some(1024 * 1024 * 50),
            max_dbs: Some(10),
            no_lmdb_sync: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MempoolSettings {
    pub filter: Option<MempoolFilterSettings>,
    pub blacklist_limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MempoolFilterSettings {
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub tx_fee: Coin,
    #[serde(default)]
    pub tx_fee_per_byte: f64,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub tx_value: Coin,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub tx_value_total: Coin,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub contract_fee: Coin,
    #[serde(default)]
    pub contract_fee_per_byte: f64,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub contract_value: Coin,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_coin")]
    pub creation_fee: Coin,
    #[serde(default)]
    pub creation_fee_per_byte: f64,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub creation_value: Coin,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub recipient_balance: Coin,
    #[serde(deserialize_with = "deserialize_coin")]
    #[serde(default)]
    pub sender_balance: Coin,
}

impl From<MempoolSettings> for MempoolConfig {
    fn from(mempool: MempoolSettings) -> Self {
        Self {
            filter_limit: mempool.blacklist_limit
                .unwrap_or(MempoolFilter::DEFAULT_BLACKLIST_SIZE),
            filter_rules: mempool.filter
                .map(MempoolRules::from)
                .unwrap_or_default(),
        }
    }
}

/// Convert mempool settings
impl From<MempoolFilterSettings> for MempoolRules {
    fn from(f: MempoolFilterSettings) -> MempoolRules {
        Self {
            tx_fee: f.tx_fee,
            tx_fee_per_byte: f.tx_fee_per_byte,
            tx_value: f.tx_value,
            tx_value_total: f.tx_value_total,
            contract_fee: f.contract_fee,
            contract_fee_per_byte: f.contract_fee_per_byte,
            contract_value: f.contract_value,
            creation_fee: f.creation_fee,
            creation_fee_per_byte: f.creation_fee_per_byte,
            creation_value: f.creation_value,
            sender_balance: f.sender_balance,
            recipient_balance: f.recipient_balance,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ValidatorSettings {
    pub key_file: Option<String>,
}
