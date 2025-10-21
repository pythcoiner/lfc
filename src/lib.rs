pub mod channel;
pub mod channel_state;
pub mod cli;
pub mod round;

use std::io::{self};

use lazy_static::lazy_static;
use miniscript::bitcoin::{
    consensus,
    secp256k1::{self, All},
    Address, Amount, Network, Transaction,
};
use thiserror::Error;

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

#[derive(Debug, Error)]
#[repr(u8)]
pub enum Error {
    #[error("Invalid derivation index: {0}")]
    DerivationIndex(u32) = 0,
    #[error("Failed to parse configuration path: {0}")]
    ParseConfigPath(String),
    #[error("Failed to parse the configuration file")]
    ParseConfig,
    #[error("Configuration file does not exists: {0}")]
    ConfigNotExists(String),
    #[error("Path for configuration file is not a file")]
    ConfigNotFile,
    #[error("Failed to open configuration file")]
    OpenConfig,
    #[error("Failed to read the configuration file")]
    ReadConfig,
    #[error("Invalid mnemonic")]
    Mnemonic,
    #[error("Invalid Xpriv")]
    Xpriv,
    #[error("Failed to derive xpriv")]
    DeriveXpriv,
    #[error("Failed to write config to file")]
    DumpConfig,
    #[error("The funding transaction must have a single funding output")]
    FundingTx,
    #[error("Rounds had already been generated")]
    RoundsAlreadyGenerated,
    #[error("Failed to signed at round {0}")]
    RoundSigning(usize),
    #[error("Fail to get unlock transaction for this round and next one! (rounds {0} & {1})")]
    FailsUnlock(usize, usize),
    #[error("No more round to unlock!")]
    UnlockEnd,
    #[error("Rounds have not yet been generated")]
    RoundsEmpty,
    #[error("Address {0} is not valid for network {1}")]
    InvalidNetworkAddress(Address, Network),
    #[error("Unsufficient balance ({0}) for send {1}")]
    UnsufficientBalance(Amount, Amount),
    #[error("I/O error: {0:?}")]
    IO(io::Error),
    #[error("Serde error: {0:?}")]
    SerdeJson(serde_json::Error),
}

impl From<Error> for serde_json::Value {
    fn from(e: Error) -> Self {
        let mut map = serde_json::Map::new();
        map.insert("error".to_string(), e.to_string().into());
        map.insert("error_id".to_string(), e.id().into());
        serde_json::Value::Object(map)
    }
}

impl Error {
    fn id(&self) -> u8 {
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
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
