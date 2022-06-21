use std::convert::TryFrom;
use std::fmt;

use bitcoin::network::constants::Network;

use lightning::ln::msgs::NetAddress;
use lightning::ln::{PaymentPreimage, PaymentSecret};

use anyhow::{bail, Context};
use argh::FromArgs;

use crate::types::Port;

#[derive(FromArgs, PartialEq, Debug)]
/// Arguments accepted by a Lexe node
pub struct LexeArgs {
    #[argh(positional)]
    /// bitcoind rpc info, in the format <username>:<password>@<host>:<-port>
    bitcoind_rpc: String,

    #[argh(option, default = "9735")]
    /// the port on which to accept Lightning P2P connections
    peer_port: Port,

    #[argh(option)]
    /// this node's Lightning Network alias
    announced_node_name: Option<String>,

    #[argh(option, default = "String::from(\"testnet\")")]
    /// testnet or mainnet. Defaults to testnet.
    network: String,

    #[argh(option, default = "999")] // TODO actually use the port
    /// the port warp uses to accept TLS connections from the owner
    warp_port: Port,
}

#[allow(dead_code)]
pub struct LdkArgs {
    pub bitcoind_rpc: BitcoindRpcInfo,
    pub peer_port: u16,
    pub ldk_announced_listen_addr: Vec<NetAddress>,
    pub ldk_announced_node_name: [u8; 32],
    pub network: Network,
    pub warp_port: u16,
}

impl TryFrom<LexeArgs> for LdkArgs {
    type Error = anyhow::Error;

    fn try_from(lexe_args: LexeArgs) -> Result<Self, Self::Error> {
        let bitcoind_rpc = lexe_args
            .bitcoind_rpc
            .try_into()
            .context("Could not parse bitcoind rpc args")?;

        let ldk_announced_node_name = match lexe_args.announced_node_name {
            Some(name) => {
                if name.len() > 32 {
                    bail!("Node Alias can not be longer than 32 bytes");
                }
                let mut bytes = [0; 32];
                bytes[..name.len()].copy_from_slice(name.as_bytes());
                bytes
            }
            None => [0; 32],
        };

        let network = match lexe_args.network {
            n if n == "testnet" => Network::Testnet,
            // NOTE: Disable mainnet for now
            // n if n == "mainnet" || n == "bitcoin" => Network::Bitcoin,
            n => bail!("Network `{}` is not supported.", n),
        };

        let ldk_info = LdkArgs {
            bitcoind_rpc,
            peer_port: lexe_args.peer_port,
            ldk_announced_listen_addr: Vec::new(),
            ldk_announced_node_name,
            network,
            warp_port: lexe_args.warp_port,
        };

        Ok(ldk_info)
    }
}

/// The information required to connect to a bitcoind instance via RPC
pub struct BitcoindRpcInfo {
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: Port,
}

impl TryFrom<String> for BitcoindRpcInfo {
    type Error = anyhow::Error;

    fn try_from(info: String) -> Result<Self, Self::Error> {
        let parts: Vec<&str> = info.rsplitn(2, '@').collect();
        if parts.len() != 2 {
            bail!("ERROR: bad bitcoind RPC URL provided");
        }
        let rpc_user_and_password: Vec<&str> = parts[1].split(':').collect();
        if rpc_user_and_password.len() != 2 {
            bail!("ERROR: bad bitcoind RPC username/password combo provided");
        }
        let username = rpc_user_and_password[0].to_string();
        let password = rpc_user_and_password[1].to_string();
        let path: Vec<&str> = parts[0].split(':').collect();
        if path.len() != 2 {
            bail!("ERROR: bad bitcoind RPC path provided");
        }
        let host = path[0].to_string();
        let port = path[1].parse::<u16>().unwrap();

        let bitcoind_rpc = BitcoindRpcInfo {
            username,
            password,
            host,
            port,
        };

        Ok(bitcoind_rpc)
    }
}

pub struct NodeAlias<'a>(pub &'a [u8; 32]);

impl fmt::Display for NodeAlias<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let alias = self
            .0
            .iter()
            .map(|b| *b as char)
            .take_while(|c| *c != '\0')
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect::<String>();
        write!(f, "{}", alias)
    }
}

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: MillisatAmount,
}

pub enum HTLCStatus {
    Pending,
    Succeeded,
    Failed,
}

pub struct MillisatAmount(pub Option<u64>);

impl fmt::Display for MillisatAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(amt) => write!(f, "{}", amt),
            None => write!(f, "unknown"),
        }
    }
}
