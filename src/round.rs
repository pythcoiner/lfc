use miniscript::{
    bitcoin::{Amount, OutPoint, Psbt, Transaction, TxOut},
    psbt::PsbtExt,
};
use serde::{Deserialize, Serialize};

use crate::SECP;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Round {
    // Psbt of this round, already signed
    psbt: Psbt,
    // signed transaction
    signed: Option<Transaction>,
    // The transaction that have unlocked this round
    spend: Option<Transaction>,
    // Spending txs (ancestors of `coins`)
    transactions: Vec<Transaction>,
    // Coins spendable by the spend key
    coins: Vec<OutPoint>,
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

impl Round {
    pub fn new(psbt: Psbt, index: u32) -> Self {
        assert!(index > 0); // 0 is funding round
        let active = index == 1;
        Self {
            psbt,
            signed: None,
            spend: None,
            transactions: Vec::new(),
            coins: Vec::new(),
            unlock: None,
            unlocked: None,
            index,
            active,
            closed: false,
        }
    }

    pub fn sign(&mut self, sign_fn: fn(&mut Psbt)) -> bool {
        sign_fn(&mut self.psbt);
        if self.psbt.finalize_mut(&SECP).is_ok() {
            let tx = self.psbt.clone().extract_tx_unchecked_fee_rate();
            self.signed = Some(tx);
            true
        } else {
            false
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
            block_height > height
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

    pub fn is_spendable(&self) -> bool {
        self.is_unlocked() && !self.coins.is_empty()
    }

    fn tx_for_coin(&self, coin: &OutPoint) -> Option<&Transaction> {
        let mut tx = None;
        self.transactions.iter().for_each(|t| {
            if t.compute_txid() == coin.txid {
                tx = Some(t);
            }
        });
        tx
    }

    pub fn spendable_amount(&self) -> Amount {
        let mut amount = Amount::ZERO;
        self.spendable_coins().iter().for_each(|c| {
            amount += c.value;
        });
        amount
    }

    pub fn spendable_coins(&self) -> Vec<TxOut> {
        let mut coins = Vec::new();
        self.coins.iter().for_each(|c| {
            let tx = self.tx_for_coin(c).unwrap();
            coins.push(tx.output[c.vout as usize].clone());
        });
        coins
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rounds(Vec<Round>);

impl Rounds {
    pub fn new() -> Self {
        let rounds = Vec::new();
        Rounds(rounds)
    }

    pub fn init(&mut self) {
        self.sort();
        // sanity check
        if !self.is_empty() {
            let mut last = 0u32;
            let mut active = 0usize;
            self.0.iter().for_each(|r| {
                assert!(r.index == last + 1);
                last += 1;
                if r.is_active() {
                    active += 1;
                }
                assert!(active < 2);
            });
        }
    }

    pub fn is_unlocked(&self) -> bool {
        if self.0.is_empty() {
            false
        } else {
            self.0[0].is_unlocked()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| a.index.cmp(&b.index));
    }

    pub fn current_round_index(&mut self) -> usize {
        //sort & sanity check
        self.init();
        for (i, r) in self.0.iter().enumerate() {
            if r.is_active() {
                return i;
            }
        }
        unreachable!();
    }

    pub fn at(&mut self, pos: usize) -> Option<&mut Round> {
        if pos < self.0.len() {
            Some(&mut self.0[pos])
        } else {
            None
        }
    }

    pub fn push(&mut self, round: Round) {
        self.0.push(round);
    }

    pub fn spendable_coins(&self) -> Vec<TxOut> {
        let mut coins = Vec::new();
        self.0.iter().for_each(|r| {
            coins.append(&mut r.spendable_coins());
        });
        coins
    }

    pub fn spendable_amount(&self) -> Amount {
        let mut coins = Amount::ZERO;
        self.0.iter().for_each(|r| {
            coins += r.spendable_amount();
        });
        coins
    }

    pub fn unlock(&mut self, chain_tip: u64) -> Option<Transaction> {
        if self.0.is_empty() {
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

    fn register_unlock(&mut self, tx: Transaction, block_height: u64, timelock: u64) -> bool {
        let index = self.current_round_index();
        let round = self.at(index).unwrap();
        let unlocked = round.register_unlocked(tx, block_height);

        if let (true, Some(next)) = (unlocked, self.at(index + 1)) {
            next.register_unlock_height(block_height + timelock);
        }
        unlocked
    }

    fn register_spend(&mut self, _tx: Transaction) -> bool {
        // TODO: iterate over all active rounds and update state
        todo!()
    }

    pub fn register(
        &mut self,
        tx: Transaction,
        block_height: u64,
        timelock: u64,
    ) -> (bool /* unlock */, bool /* spend */) {
        let index = self.current_round_index();
        let round = self.at(index).unwrap();
        let unlocked = if round.is_unlockable(block_height) {
            self.register_unlock(tx.clone(), block_height, timelock)
        } else {
            false
        };
        let spend = if !unlocked {
            self.register_spend(tx)
        } else {
            false
        };
        (unlocked, spend)
    }

    pub fn as_mut_vec(&mut self) -> &mut Vec<Round> {
        &mut self.0
    }
}

impl Default for Rounds {
    fn default() -> Self {
        Self::new()
    }
}
