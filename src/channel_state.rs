use std::{fs::File, io::Read, path::PathBuf, str::FromStr};

use bip39::Mnemonic;
use miniscript::{
    bitcoin::{
        bip32::{ChildNumber, DerivationPath, Xpriv, Xpub},
        Network,
    },
    descriptor::{DescriptorXKey, Wildcard},
    DescriptorPublicKey,
};
use serde::{Deserialize, Serialize};

use crate::{round::Rounds, Error, SECP};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelState {
    /// Mnemonic of the covenant locking/unlocking policy
    pub cov_mnemonic: String,
    /// Mnemonic of the spend policy
    pub spend_mnemonic: String,
    /// Max amount spendable at each round (sats)
    pub amount: u64,
    /// Delay between 2 round in blocks (nSequence)
    pub delay: u16,
    /// Account index
    pub account: u32,
    /// Bitcoin network
    pub network: Network,
    /// Rounds metadata
    #[serde(skip_serializing_if = "Rounds::is_empty", default)]
    pub rounds: Rounds,
    #[serde(skip)]
    pub path: PathBuf,
}

impl ChannelState {
    pub fn origin_path(&self) -> Vec<ChildNumber> {
        vec![84, 1, self.account]
            .into_iter()
            .map(|c| ChildNumber::from_hardened_idx(c).unwrap())
            .collect()
    }

    pub fn from_file(path: &str) -> Result<Self, Error> {
        let path = PathBuf::from_str(path).map_err(|_| Error::ParseConfigPath)?;
        if !path.exists() {
            return Err(Error::ConfigNotExists);
        } else if !path.is_file() {
            return Err(Error::ConfigNotFile);
        }
        let mut file = File::open(path).map_err(|_| Error::OpenConfig)?;
        let mut conf_str = String::new();
        let _conf_size = file
            .read_to_string(&mut conf_str)
            .map_err(|_| Error::ReadConfig)?;
        let mut conf: Self = serde_json::from_str(&conf_str).map_err(|_| Error::ParseConfig)?;
        // sort & sanity check
        conf.rounds.init();
        Ok(conf)
    }

    pub fn to_file(&self) -> Result<(), Error> {
        let file = File::create(&self.path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    fn master_xpriv(mnemonic: &str) -> Result<Xpriv, Error> {
        let mnemonic = Mnemonic::from_str(mnemonic).map_err(|_| Error::Mnemonic)?;
        let seed = mnemonic.to_seed("");
        Xpriv::new_master(Network::Regtest, &seed).map_err(|_| Error::Xpriv)
    }

    fn derived_xpriv(xpriv: Xpriv, path: Vec<ChildNumber>) -> Result<Xpriv, Error> {
        xpriv
            .derive_priv(&SECP, &path)
            .map_err(|_| Error::DeriveXpriv)
    }

    pub fn cov_xpriv_at(&self, sub_account: u32, index: u32) -> Xpriv {
        let xpriv = self.cov_master_xpriv().unwrap();
        let derived = Self::derived_xpriv(xpriv, self.origin_path()).unwrap();
        let path = vec![
            ChildNumber::from_normal_idx(sub_account).unwrap(),
            ChildNumber::from_normal_idx(index).unwrap(),
        ];
        derived.derive_priv(&SECP, &path).unwrap()
    }

    pub fn spend_xpriv_at(self, sub_account: u32, index: u32) -> Xpriv {
        let xpriv = self.spend_master_xpriv().unwrap();
        let derived = Self::derived_xpriv(xpriv, self.origin_path()).unwrap();
        let path = vec![
            ChildNumber::from_normal_idx(sub_account).unwrap(),
            ChildNumber::from_normal_idx(index).unwrap(),
        ];
        derived.derive_priv(&SECP, &path).unwrap()
    }

    fn cov_master_xpriv(&self) -> Result<Xpriv, Error> {
        Self::master_xpriv(&self.cov_mnemonic)
    }

    fn spend_master_xpriv(&self) -> Result<Xpriv, Error> {
        Self::master_xpriv(&self.spend_mnemonic)
    }

    fn xpub(&self, master_xpriv: Xpriv, sub_account: u32) -> Result<DescriptorPublicKey, Error> {
        let fg = master_xpriv.fingerprint(&SECP);
        let xpriv = Self::derived_xpriv(master_xpriv, self.origin_path())?;
        let xpub = Xpub::from_priv(&SECP, &xpriv);

        let key = DescriptorXKey {
            origin: Some((fg, self.origin_path().into())),
            xkey: xpub,
            derivation_path: DerivationPath::from_iter(vec![sub_account.into()]),
            wildcard: Wildcard::Unhardened,
        };
        Ok(DescriptorPublicKey::XPub(key))
    }

    pub fn cov_xpub(&self, sub_account: u32) -> Result<DescriptorPublicKey, Error> {
        self.xpub(self.cov_master_xpriv()?, sub_account)
    }

    pub fn spend_xpub(&self, sub_account: u32) -> Result<DescriptorPublicKey, Error> {
        self.xpub(self.spend_master_xpriv()?, sub_account)
    }

    pub fn delay(&self) -> u16 {
        self.delay
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }
}
