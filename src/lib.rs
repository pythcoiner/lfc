pub mod channel;
pub mod channel_state;
pub mod cli;
pub mod round;

use std::io::{self};

use lazy_static::lazy_static;
use miniscript::bitcoin::{
    consensus,
    secp256k1::{self, All},
    Network, Transaction,
};

lazy_static! {
    pub static ref SECP: secp256k1::Secp256k1<All> = secp256k1::Secp256k1::new();
}

pub const FEE: u64 = 600;
pub const MAX_DERIV: u32 = (2u64.pow(31) - 1) as u32;
pub const DEFAULT_NETWORK: Network = Network::Regtest;

pub fn parse_tx(raw_tx: &str) -> Result<Transaction, String> {
    let tx: Result<Transaction, _> = consensus::encode::deserialize_hex(raw_tx);
    tx.map_err(|e| e.to_string())
}

#[derive(Debug)]
pub enum Error {
    ParseConfigPath,
    ParseConfig,
    ConfigNotExists,
    ConfigNotFile,
    OpenConfig,
    ReadConfig,
    Mnemonic,
    Xpriv,
    DeriveXpriv,
    DumpConfig,
    IO(io::Error),
    SerdeJson(serde_json::Error),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::IO(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::SerdeJson(value)
    }
}

#[cfg(test)]
mod tests {

    use std::path::PathBuf;

    use bip39::Mnemonic;

    use crate::{channel_state::ChannelState, round::Rounds};

    use super::*;

    #[test]
    fn serialize_conf() {
        let master_mnemonic = Mnemonic::generate(12).unwrap().to_string();
        let spend_mnemonic = Mnemonic::generate(12).unwrap().to_string();

        let conf = ChannelState {
            master_mnemonic,
            spend_mnemonic,
            amount: 10_000_000,
            delay: 4500,
            account: 0,
            network: DEFAULT_NETWORK,
            rounds: Rounds::new(),
            path: PathBuf::new(),
        };

        let conf_str = serde_json::to_string_pretty(&conf).unwrap();
        let parsed: ChannelState = serde_json::from_str(&conf_str).unwrap();
        assert_eq!(conf, parsed);
    }
}
