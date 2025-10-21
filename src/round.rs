use std::{collections::BTreeMap, fmt::Debug};

use miniscript::{
    bitcoin::{
        absolute::{Height, LockTime},
        bip32::ChildNumber,
        psbt::{Input, Output},
        transaction::Version,
        Address, Amount, Network, OutPoint, Psbt, ScriptBuf, Transaction, TxOut, Txid,
    },
    psbt::PsbtExt,
    Descriptor, DescriptorPublicKey,
};
use serde::{Deserialize, Serialize};

use crate::{channel::txin, Error, SECP};

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Round {
    // Psbt of this round, already signed
    psbt: Psbt,
    // signed transaction
    signed: Option<Transaction>,
    // The transaction that have unlocked this round
    spend: Option<Transaction>,
    // Blockheight we must wait to unlock
    unlock: Option<u64>,
    // Blockheight of the block containing the unlock tx
    unlocked: Option<u64>,
    // Index of the round
    index: u32,
    // Current_round
    active: bool,
    // Next round have been unlocked
    closed: bool,
}

impl Debug for Round {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Round")
            .field("psbt", &"[redacted]".to_string())
            .field("signed_tx", &self.signed.is_some())
            .field("spend_tx", &self.spend.is_some())
            .field("unlock", &self.unlock)
            .field("unlocked", &self.unlocked)
            .field("index", &self.index)
            .field("active", &self.active)
            .field("closed", &self.closed)
            .finish()
    }
}

impl Round {
    pub fn new(psbt: Psbt, index: u32) -> Self {
        assert!(index > 0); // 0 is funding round
        let active = index == 1;
        Self {
            psbt,
            signed: None,
            spend: None,
            unlock: None,
            unlocked: None,
            index,
            active,
            closed: false,
        }
    }

    pub fn sign<S>(&mut self, sign_fn: S) -> bool
    where
        S: Fn(&mut Psbt),
    {
        sign_fn(&mut self.psbt);
        match self.psbt.finalize_mut(&SECP) {
            Ok(_) => {
                let tx = self.psbt.clone().extract_tx_unchecked_fee_rate();
                self.signed = Some(tx);
                true
            }
            Err(_) => false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    pub fn is_unlockable(&self, block_height: u64) -> bool {
        let timelocked = if let Some(height) = self.unlock {
            block_height >= height
        } else {
            self.index == 1
        };
        timelocked && self.signed.is_some()
    }

    pub fn unlock(&mut self) -> Option<Transaction> {
        self.signed.clone()
    }

    pub fn unlock_after(&self) -> Option<u64> {
        self.unlock
    }

    pub fn is_unlocked(&self) -> bool {
        self.spend.is_some()
    }

    pub fn register_unlocked(&mut self, tx: Transaction, block_height: u64) -> bool {
        let taken = self.signed.take();
        if let Some(signed) = taken {
            let registered: bool;
            if signed != tx {
                self.signed = Some(signed);
                registered = false;
            } else if self.unlocked.is_none() && self.spend.is_none() {
                self.unlocked = Some(block_height);
                self.spend = Some(tx);
                registered = true;
            } else {
                self.signed = Some(signed);
                unreachable!()
            }
            registered
        } else {
            false
        }
    }

    pub fn register_spend(&mut self, _tx: Transaction) {
        //TODO:
    }

    pub fn register_unlock_height(&mut self, block_height: u64) {
        self.unlock = Some(block_height);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Coin {
    pub outpoint: OutPoint,
    pub prevout: TxOut,
    pub index: ChildNumber,
    pub spent: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rounds {
    rounds: Vec<Round>,
    transactions: BTreeMap<Txid, Transaction>,
    coins: BTreeMap<OutPoint, Coin>,
    spend_index: u32,
}

impl Rounds {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn init(&mut self) {
        self.sort();
        // sanity check
        if !self.is_empty() {
            let mut last = 0u32;
            let mut active = 0usize;
            self.rounds.iter().for_each(|r| {
                assert!(r.index == last + 1);
                last += 1;
                if r.is_active() {
                    active += 1;
                }
                assert!(active < 2);
            });
        }
    }

    pub fn count(&self) -> usize {
        self.rounds.len()
    }

    pub fn set_spend_index(&mut self, index: u32) {
        // we can only increment this index
        if index > self.spend_index {
            self.spend_index = index;
        }
    }

    pub fn spend_index(&self) -> u32 {
        self.spend_index
    }

    pub fn is_unlocked(&self) -> bool {
        if self.rounds.is_empty() {
            false
        } else {
            self.rounds[0].is_unlocked()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
    }

    pub fn sort(&mut self) {
        self.rounds.sort_by(|a, b| a.index.cmp(&b.index));
    }

    pub fn current_round_index(&mut self) -> usize {
        //sort & sanity check
        self.init();
        for (i, r) in self.rounds.iter().enumerate() {
            if r.is_active() {
                return i;
            }
        }
        unreachable!();
    }

    pub fn at(&mut self, pos: usize) -> Option<&mut Round> {
        if pos < self.rounds.len() {
            Some(&mut self.rounds[pos])
        } else {
            None
        }
    }

    pub fn push(&mut self, round: Round) {
        self.rounds.push(round);
    }

    pub fn spendable_coins(&self) -> Vec<Coin> {
        self.coins
            .iter()
            .filter_map(|(_, c)| (!c.spent).then_some(c.clone()))
            .collect()
    }

    pub fn spendable_amount(&self) -> Amount {
        self.spendable_coins()
            .into_iter()
            .fold(Amount::ZERO, |a, b| a + b.prevout.value)
    }

    pub fn transactions(&self) -> BTreeMap<Txid, Transaction> {
        self.transactions.clone()
    }

    pub fn insert_transaction(&mut self, tx: Transaction) {
        self.transactions.insert(tx.compute_txid(), tx);
    }

    pub fn unlock(&mut self, chain_tip: u64) -> Option<Transaction> {
        if self.rounds.is_empty() {
            return None;
        }
        let index = self.current_round_index();

        // first round
        let unlocked = {
            let round = self.at(index).unwrap();
            if round.is_unlockable(chain_tip) {
                return round.unlock();
            } else {
                round.is_unlocked()
            }
        };

        // all others rounds
        if let (true, Some(next)) = (unlocked, self.at(index + 1)) {
            if next.is_unlockable(chain_tip) {
                next.set_active(true);
                let unlock = next.unlock();
                #[allow(dropping_references)]
                drop(next);
                self.at(index).unwrap().set_active(false);
                unlock
            } else {
                None
            }
        } else {
            None
        }
    }

    fn try_register_unlock<'a, D>(
        &mut self,
        tx: Transaction,
        block_height: u64,
        delay: u64,
        derive: &D,
    ) -> bool
    where
        D: Fn(ChildNumber) -> ScriptBuf + 'a,
    {
        let index = self.current_round_index();

        let current_unlocked = self.at(index).unwrap().is_unlocked();
        let unlocked = if !current_unlocked {
            // We try unlock current round
            let round = self.at(index).unwrap();
            let unlocked = if round.is_unlockable(block_height) {
                round.register_unlocked(tx.clone(), block_height)
            } else {
                false
            };
            if let (true, Some(next)) = (unlocked, self.at(index + 1)) {
                next.register_unlock_height(block_height + delay);
            }
            unlocked
        } else if let Some(next) = self.at(index + 1) {
            // We try unlock the next round
            if !next.is_unlocked() && next.is_unlockable(block_height) {
                let ul = next.register_unlocked(tx.clone(), block_height);
                if ul {
                    next.set_active(true);
                    self.at(index).expect("current round").set_active(false);
                    if let (true, Some(next_next)) = (ul, self.at(index + 2)) {
                        next_next.register_unlock_height(block_height + delay);
                    }
                }
                ul
            } else {
                false
            }
        } else {
            false
        };
        if unlocked {
            let mut spk_index_map = BTreeMap::new();
            for i in 0..self.rounds.len() + 1 {
                let i = ChildNumber::from_normal_idx(i as u32).unwrap();
                let spk = derive(i);
                spk_index_map.insert(spk, i);
            }

            let txid = tx.compute_txid();
            for (i, out) in tx.output.iter().enumerate() {
                if let Some(index) = spk_index_map.get(&out.script_pubkey).cloned() {
                    let op = OutPoint {
                        txid,
                        vout: i as u32,
                    };
                    self.coins.insert(
                        op,
                        Coin {
                            prevout: out.clone(),
                            index,
                            spent: false,
                            outpoint: op,
                        },
                    );
                }
            }

            self.transactions.insert(tx.compute_txid(), tx);
        }
        unlocked
    }

    fn try_register_spend<'a, D>(&mut self, tx: Transaction, derive: &D) -> bool
    where
        D: Fn(ChildNumber) -> ScriptBuf + 'a,
    {
        let mut spend = false;
        // register spent coins
        for i in &tx.input {
            if let Some(coin) = self.coins.get_mut(&i.previous_output) {
                if coin.spent {
                    println!("Spend transaction try to spend an already spent coin!");
                    return false;
                }
                coin.spent = true;
                spend = true;
            } else {
                println!("Spend transaction can only spend owned coins!");
                return false;
            }
        }
        // register newly created owned coins
        let txid = tx.compute_txid();

        // we look ahead of the current spend index to find changes coins
        let mut spk_index_map = BTreeMap::new();
        for index in 0..self.spend_index() + 100 {
            let index = ChildNumber::from_normal_idx(index).unwrap();
            let script = derive(index);
            spk_index_map.insert(script, index);
        }

        for (vout, o) in tx.output.iter().enumerate() {
            if let Some(index) = spk_index_map.get(&o.script_pubkey) {
                // This coin is owned
                let prevout = OutPoint {
                    txid,
                    vout: vout as u32,
                };
                self.coins.insert(
                    prevout,
                    Coin {
                        prevout: o.clone(),
                        index: *index,
                        spent: false,
                        outpoint: prevout,
                    },
                );
            }
        }
        if spend {
            self.insert_transaction(tx);
        }
        spend
    }

    pub fn register<'a, D>(
        &mut self,
        tx: Transaction,
        block_height: u64,
        delay: u64,
        derive: D,
    ) -> (bool /* unlock */, bool /* spend */)
    where
        D: Fn(ChildNumber) -> ScriptBuf + 'a,
    {
        let unlocked = self.try_register_unlock(tx.clone(), block_height, delay, &derive);

        let spend = if !unlocked {
            self.try_register_spend(tx, &derive)
        } else {
            false
        };

        (unlocked, spend)
    }

    pub fn craft_spend(
        &self,
        addr: Address,
        amount: Amount,
        spend_descriptor: Descriptor<DescriptorPublicKey>,
    ) -> Result<Psbt, Error> {
        // FIXME: do not hardcode fees
        const FEE: Amount = Amount::from_sat(150);

        let spendable = self.spendable_amount();
        let mut change_amt = if spendable < (amount + FEE) {
            return Err(Error::UnsufficientBalance(spendable, amount));
        } else {
            spendable - amount - FEE
        };

        // DUST
        if change_amt < Amount::from_sat(500) {
            change_amt = Amount::ZERO;
        }

        let change_index = ChildNumber::from_normal_idx(self.spend_index() + 1).unwrap();
        let change_script = spend_descriptor
            .at_derivation_index(change_index.into())
            .unwrap()
            .address(Network::Regtest)
            .unwrap()
            .script_pubkey();
        let change = TxOut {
            value: change_amt,
            script_pubkey: change_script,
        };
        let send = TxOut {
            value: amount,
            script_pubkey: addr.script_pubkey(),
        };

        let inputs_coins = self.spendable_coins();
        let inputs = inputs_coins
            .iter()
            .map(|c| -> _ { txin(c.outpoint, 0) })
            .collect();
        let (outputs, change_pos) = if change_amt != Amount::ZERO {
            (vec![change, send], Some(0usize))
        } else {
            (vec![send], None)
        };

        let tx = Transaction {
            version: Version(2),
            lock_time: LockTime::Blocks(Height::ZERO),
            input: inputs,
            output: outputs,
        };
        let mut psbt_inputs = vec![];
        let mut input_indexes = vec![];
        for inp in inputs_coins {
            let mut i = Input::default();
            let op = inp.outpoint;
            let funding_tx = self.transactions.get(&op.txid).unwrap();
            i.witness_utxo = Some(funding_tx.output[op.vout as usize].clone());
            psbt_inputs.push(i);
            input_indexes.push(inp.index);
        }

        let mut psbt_outputs = vec![];
        for _ in 0..tx.output.len() {
            psbt_outputs.push(Output::default());
        }

        let mut psbt = Psbt {
            unsigned_tx: tx,
            version: 0,
            xpub: BTreeMap::new(),
            proprietary: Default::default(),
            unknown: BTreeMap::new(),
            inputs: psbt_inputs,
            outputs: psbt_outputs,
        };

        // Populate inputs metadata
        for (index, child) in input_indexes.iter().enumerate() {
            let input_descriptor = spend_descriptor
                .at_derivation_index((*child).into())
                .unwrap();
            PsbtExt::update_input_with_descriptor(&mut psbt, index, &input_descriptor).unwrap();
        }

        // Populate change output metadata
        if let Some(pos) = change_pos {
            let change_descriptor = spend_descriptor
                .at_derivation_index(change_index.into())
                .unwrap();
            PsbtExt::update_output_with_descriptor(&mut psbt, pos, &change_descriptor).unwrap();
        }

        Ok(psbt)
    }

    pub fn as_mut_vec(&mut self) -> &mut Vec<Round> {
        &mut self.rounds
    }
}
