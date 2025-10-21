use std::{collections::BTreeMap, fs::File, io::Read, path::PathBuf, str::FromStr};

use bip39::Mnemonic;
use miniscript::{
    bitcoin::{
        address::NetworkUnchecked,
        bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub},
        Address, Amount, Network, Transaction, Txid,
    },
    descriptor::{DescriptorXKey, Wildcard},
    psbt::PsbtExt,
    DescriptorPublicKey,
};
use serde::{Deserialize, Serialize};

use crate::{
    channel::Channel,
    round::{Coin, Round, Rounds},
    Error, SECP,
};

#[derive(Debug, Clone, PartialEq, Error)]
pub enum Registered {
    #[error("Unlock registered!")]
    Unlock,
    #[error("Spend registered!")]
    Spend,
    #[error("Transaction has not been registered!")]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelState {
    /// Mnemonic of the master locking/unlocking policy
    pub master_mnemonic: String,
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
        let path = PathBuf::from_str(path).map_err(|_| Error::ParseConfigPath(path.to_string()))?;
        if !path.exists() {
            return Err(Error::ConfigNotExists(path.to_str().unwrap().to_string()));
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

    pub fn funding_address(&self) -> Address {
        let channel = Channel::from_state(self);
        channel.master_addr(0)
    }

    pub fn channel(&self) -> Channel {
        Channel::from_state(self)
    }

    pub fn generate_rounds(
        &mut self,
        funding_tx: Transaction,
        fee: u64,
        verbose: bool,
    ) -> Result<(), Error> {
        if self.rounds.count() > 0 {
            return Err(Error::RoundsAlreadyGenerated);
        }

        // We check there is only one output funding the funding address
        let funding_addr = self.funding_address();
        let mut funding = None;
        for (pos, txout) in funding_tx.output.iter().enumerate() {
            if txout.script_pubkey == funding_addr.script_pubkey() {
                if funding.is_none() {
                    funding = Some((pos, txout.value));
                } else {
                    return Err(Error::FundingTx);
                }
            }
        }

        let (pos, amount) = funding.ok_or(Error::FundingTx)?;
        let amount = amount.to_sat();

        let mut txs = Vec::new();
        let mut previous_tx = funding_tx.clone();
        let mut previous_amount = amount;
        let mut index = 1;

        loop {
            let (spend, relock) = if previous_amount > self.amount {
                let mut relock = previous_amount.saturating_sub(self.amount);
                if relock <= fee {
                    relock = 0;
                }
                let spend = if relock == 0 {
                    previous_amount.saturating_sub(fee)
                } else {
                    self.amount
                };
                (spend, relock)
            } else {
                (previous_amount - fee, 0)
            };
            let funding_pos = if index == 1 { pos } else { 0 };
            let psbt = self.channel().craft_round(
                previous_tx.clone(),
                index,
                spend,
                relock,
                funding_pos,
                verbose,
            );
            previous_tx = psbt.unsigned_tx.clone();
            previous_amount = previous_tx.output[0].value.to_sat();
            index += 1;

            txs.push(psbt);

            if relock < (2 * fee) {
                break;
            }
        }

        let mut index = 0;
        #[allow(clippy::explicit_counter_loop)]
        for psbt in txs {
            index += 1;
            let round = Round::new(psbt, index);
            self.rounds.push(round);
        }
        self.rounds.set_spend_index(index);
        self.rounds.insert_transaction(funding_tx);
        Ok(())
    }

    pub fn sign_rounds(&mut self, verbose: bool) -> Result<(), Error> {
        if self.rounds.count() < 1 {
            return Err(Error::RoundsEmpty);
        }
        let channel = Channel::from_state(self);

        // FIXME: avoid cloning self
        let s = self.clone();

        let mut index = 0;
        let mut transactions = vec![];
        for round in self.rounds.as_mut_vec() {
            index += 1;
            if verbose {
                println!("Signing round {index}");
            }
            let success = round.sign(|psbt| {
                channel.presign_round(psbt, &s);
            });
            if !success {
                let error = Error::RoundSigning(index);
                if verbose {
                    println!("{error}");
                }
                return Err(Error::RoundSigning(index));
            }
            if let Some(tx) = round.unlock() {
                transactions.push(tx);
            }
        }
        Ok(())
    }

    pub fn unlock_next(
        &mut self,
    ) -> Result<
        (
            Transaction,
            Option<u64>, /* height */
            usize,       /* index */
            bool,        /* next */
        ),
        Error,
    > {
        let rounds = &mut self.rounds;

        let index = rounds.current_round_index();
        let current = rounds.at(index).unwrap();

        match current.unlock() {
            Some(tx) => {
                let height = current.unlock_after();
                Ok((tx, height, index, false))
            }
            None => {
                let next = rounds.at(index + 1);
                if let Some(next) = next {
                    if let Some(tx) = next.unlock() {
                        let height = next.unlock_after();
                        Ok((tx, height, index, true))
                        // on_unlock_tx(tx, height, index + 1, true);
                    } else {
                        Err(Error::FailsUnlock(index, index + 1))
                    }
                } else {
                    Err(Error::UnlockEnd)
                }
            }
        }
    }

    pub fn register(&mut self, tx: Transaction, height: u64) -> Registered {
        let channel = Channel::from_state(self);
        let delay = self.delay() as u64;
        let rounds = &mut self.rounds;

        let descriptor = channel.spend_descriptor();
        let (unlock, spend) = rounds.register(tx, height, delay, |c| {
            descriptor
                .at_derivation_index(c.into())
                .unwrap()
                .address(self.network)
                .unwrap()
                .script_pubkey()
        });
        if unlock {
            Registered::Unlock
        } else if spend {
            Registered::Spend
        } else {
            Registered::None
        }
    }

    pub fn create_spend(
        &self,
        address: Address<NetworkUnchecked>,
        amount: u64,
    ) -> Result<(Transaction, Amount, Address), Error> {
        let channel = Channel::from_state(self);
        let amount = Amount::from_sat(amount);
        if !address.is_valid_for_network(self.network) {
            return Err(Error::InvalidNetworkAddress(
                address.assume_checked(),
                self.network,
            ));
        }
        let address = address.assume_checked();
        let mut psbt =
            self.rounds
                .craft_spend(address.clone(), amount, channel.spend_descriptor())?;

        channel.sign_spend(&mut psbt, self)?;
        PsbtExt::finalize_mut(&mut psbt, &SECP).unwrap();
        let tx = psbt.extract_tx_unchecked_fee_rate();
        Ok((tx, amount, address))
    }

    pub fn spendable_amount(&self) -> Amount {
        self.rounds.spendable_amount()
    }

    pub fn spendable_coins(&self) -> Vec<Coin> {
        self.rounds.spendable_coins()
    }

    pub fn transactions(&self) -> BTreeMap<Txid, Transaction> {
        self.rounds.transactions()
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

    pub fn master_xpriv_at(&self, sub_account: u32, index: u32) -> Xpriv {
        let xpriv = self.master_master_xpriv().unwrap();
        let derived = Self::derived_xpriv(xpriv, self.origin_path()).unwrap();
        let path = vec![
            ChildNumber::from_normal_idx(sub_account).unwrap(),
            ChildNumber::from_normal_idx(index).unwrap(),
        ];
        derived.derive_priv(&SECP, &path).unwrap()
    }

    pub fn spend_xpriv_at(&self, sub_account: u32, index: u32) -> Xpriv {
        let xpriv = self.spend_master_xpriv().unwrap();
        let derived = Self::derived_xpriv(xpriv, self.origin_path()).unwrap();
        let path = vec![
            ChildNumber::from_normal_idx(sub_account).unwrap(),
            ChildNumber::from_normal_idx(index).unwrap(),
        ];
        derived.derive_priv(&SECP, &path).unwrap()
    }

    fn master_master_xpriv(&self) -> Result<Xpriv, Error> {
        Self::master_xpriv(&self.master_mnemonic)
    }

    fn spend_master_xpriv(&self) -> Result<Xpriv, Error> {
        Self::master_xpriv(&self.spend_mnemonic)
    }

    pub fn master_fingerprint(&self) -> Result<Fingerprint, Error> {
        Ok(self.master_master_xpriv()?.fingerprint(&SECP))
    }

    pub fn spend_fingerprint(&self) -> Result<Fingerprint, Error> {
        Ok(self.spend_master_xpriv()?.fingerprint(&SECP))
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

    pub fn master_xpub(&self, sub_account: u32) -> Result<DescriptorPublicKey, Error> {
        self.xpub(self.master_master_xpriv()?, sub_account)
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
