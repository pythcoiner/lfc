use std::collections::BTreeMap;

#[allow(unused_imports)]
use miniscript::psbt::PsbtExt;
use miniscript::{
    bitcoin::{
        absolute::{Height, LockTime},
        ecdsa,
        psbt::{raw::ProprietaryKey, Input, Output},
        sighash,
        transaction::Version,
        Address, Amount, EcdsaSighashType, Network, OutPoint, Psbt, ScriptBuf, Sequence,
        Transaction, TxIn, TxOut, Witness,
    },
    descriptor::Wpkh,
    Descriptor, DescriptorPublicKey, ToPublicKey,
};

const SUB_ACCOUNT: u32 = 0;

#[macro_export]
macro_rules! round_index_key {
    () => {
        ProprietaryKey {
            prefix: b"lfc".to_vec(),
            subtype: 0x01,
            key: b"round_index".to_vec(),
        }
    };
}

use crate::{channel_state::ChannelState, Error, FEE, SECP};

pub fn txin(outpoint: OutPoint, sequence: u16) -> TxIn {
    TxIn {
        previous_output: outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::from_height(sequence),
        witness: Witness::new(),
    }
}

pub struct Channel {
    #[allow(unused)]
    master: DescriptorPublicKey,
    #[allow(unused)]
    spend: DescriptorPublicKey,
    timelock: u16,
    network: Network,
    master_descriptor: Descriptor<DescriptorPublicKey>,
    spend_descriptor: Descriptor<DescriptorPublicKey>,
}

impl Channel {
    pub fn from_state(state: &ChannelState) -> Self {
        let master = state.master_xpub(SUB_ACCOUNT).unwrap();
        let spend = state.spend_xpub(SUB_ACCOUNT).unwrap();
        let timelock = state.delay();
        let network = state.network;

        let master_descriptor = Descriptor::Wpkh(Wpkh::new(master.clone()).unwrap());

        let spend_descriptor = Descriptor::Wpkh(Wpkh::new(spend.clone()).unwrap());

        Self {
            master,
            spend,
            timelock,
            network,
            master_descriptor,
            spend_descriptor,
        }
    }

    pub fn spend_addr(&self, index: u32) -> Address {
        self.spend_descriptor
            .at_derivation_index(index)
            .expect("must not fail")
            .address(self.network)
            .expect("must not fail")
    }

    pub fn master_addr(&self, index: u32) -> Address {
        self.master_descriptor
            .at_derivation_index(index)
            .expect("must not fail")
            .address(self.network)
            .expect("must not fail")
    }

    pub fn craft_round(
        &self,
        previous_tx: Transaction,
        round_index: u32,
        spend: u64,
        relock: u64,
        funding_pos: usize,
        verbose: bool,
    ) -> Psbt {
        let vbytes = 150;
        #[allow(clippy::identity_op)]
        let fees = vbytes * 1; /* sats */
        if verbose {
            println!(
            "Generate transaction for round {round_index}, spending {spend} and relocking {relock}"
        );
        }

        // deduct fees
        let (spend, relock) = if relock < fees {
            let fees = fees - relock;
            let relock = 0;
            let spend = spend - fees;
            (spend, relock)
        } else {
            let relock = relock - fees;
            (spend, relock)
        };

        let relock = if relock < (FEE * 2) { 0 } else { relock };

        let spend = Amount::from_sat(spend);
        let relock = Amount::from_sat(relock);
        let relock_addr = self.master_addr(round_index);
        let relock_out = TxOut {
            value: relock,
            script_pubkey: relock_addr.into(),
        };
        let spend_addr = self.spend_addr(round_index);
        let spend_out = TxOut {
            value: spend,
            script_pubkey: spend_addr.into(),
        };

        let outpoint = OutPoint {
            txid: previous_tx.compute_txid(),
            vout: funding_pos as u32,
        };
        let sequence = if round_index == 1 { 0 } else { self.timelock };
        let tx_input = txin(outpoint, sequence);

        let outputs = if relock != Amount::ZERO {
            vec![relock_out, spend_out]
        } else {
            vec![spend_out]
        };

        let tx = Transaction {
            version: Version(2),
            lock_time: LockTime::Blocks(Height::ZERO),
            input: vec![tx_input],
            output: outputs,
        };
        let mut psbt_input = Input::default();

        // verify the funding pos match our key
        assert!(self
            .master_addr(round_index - 1)
            .matches_script_pubkey(&previous_tx.output[funding_pos].script_pubkey));
        let input_descriptor = self
            .master_descriptor
            .at_derivation_index(round_index - 1)
            .unwrap();

        psbt_input.witness_utxo = Some(previous_tx.output[funding_pos].clone());

        let psbt_inputs = vec![psbt_input];

        let relock_decriptor = self
            .master_descriptor
            .at_derivation_index(round_index)
            .unwrap();
        let spend_descriptor = self
            .spend_descriptor
            .at_derivation_index(round_index)
            .unwrap();
        let output_relock = Output::default();
        let output_spend = Output::default();

        let psbt_outputs = if relock != Amount::ZERO {
            vec![output_relock, output_spend]
        } else {
            vec![output_spend]
        };

        // store the index in the proprietary map
        let mut proprietary = BTreeMap::new();
        proprietary.insert(round_index_key!(), round_index.to_le_bytes().to_vec());

        let mut psbt = Psbt {
            unsigned_tx: tx,
            version: 0,
            xpub: BTreeMap::new(),
            proprietary,
            unknown: BTreeMap::new(),
            inputs: psbt_inputs,
            outputs: psbt_outputs,
        };

        // Populate input metadata
        PsbtExt::update_input_with_descriptor(&mut psbt, 0, &input_descriptor).unwrap();

        // Populate outputs metadata
        if relock != Amount::ZERO {
            PsbtExt::update_output_with_descriptor(&mut psbt, 0, &relock_decriptor).unwrap();
            PsbtExt::update_output_with_descriptor(&mut psbt, 1, &spend_descriptor).unwrap();
        } else {
            PsbtExt::update_output_with_descriptor(&mut psbt, 0, &spend_descriptor).unwrap();
        }

        psbt
    }

    pub fn presign_round(&self, psbt: &mut Psbt, state: &ChannelState) {
        // index is stored in the proprietary map
        let raw_index: [u8; 4] = psbt
            .proprietary
            .get(&round_index_key!())
            .unwrap()
            .to_vec()
            .try_into()
            .unwrap();
        let index = u32::from_le_bytes(raw_index) - 1;

        assert_eq!(psbt.inputs.len(), 1);
        // TODO: verify outputs go to the right descriptors

        let xpriv = state.master_xpriv_at(SUB_ACCOUNT, index);
        let sk = xpriv.private_key;
        let pk = sk.public_key(&SECP);

        let mut cache = sighash::SighashCache::new(psbt.unsigned_tx.clone());
        let (msg, sighash_type) = psbt.sighash_ecdsa(0, &mut cache).unwrap();
        assert_eq!(sighash_type, EcdsaSighashType::All);

        let signature = SECP.sign_ecdsa_low_r(&msg, &sk);
        let signature = ecdsa::Signature {
            signature,
            sighash_type: EcdsaSighashType::All,
        };
        psbt.inputs[0]
            .partial_sigs
            .insert(pk.to_public_key(), signature);
    }

    pub fn sign_spend(&self, psbt: &mut Psbt, state: &ChannelState) -> Result<(), Error> {
        // TODO: handle errors

        // sanity check psbt
        assert_eq!(psbt.inputs.len(), psbt.unsigned_tx.input.len());
        assert_eq!(psbt.outputs.len(), psbt.unsigned_tx.output.len());

        // TODO: check outputs structure

        let spend_fg = state.spend_fingerprint().unwrap();
        let mut cache = sighash::SighashCache::new(psbt.unsigned_tx.clone());

        for i in 0..psbt.inputs.len() {
            let deriv = psbt.inputs[i].bip32_derivation.clone();
            for (pk, (fg, deriv)) in deriv {
                if fg == spend_fg {
                    let index = *deriv.to_u32_vec().last().unwrap();
                    let sk = state.spend_xpriv_at(SUB_ACCOUNT, index).private_key;
                    let pbk = sk.public_key(&SECP);
                    assert_eq!(pk, pbk);
                    let (msg, sighash_type) = psbt.sighash_ecdsa(i, &mut cache).unwrap();
                    assert_eq!(sighash_type, EcdsaSighashType::All);
                    let signature = SECP.sign_ecdsa_low_r(&msg, &sk);
                    let signature = ecdsa::Signature {
                        signature,
                        sighash_type: EcdsaSighashType::All,
                    };
                    psbt.inputs[i]
                        .partial_sigs
                        .insert(pk.to_public_key(), signature);
                } else {
                    panic!("only owned coins are allowed in spend transactions!");
                }
            }
        }
        Ok(())
    }

    pub fn spend_descriptor(&self) -> Descriptor<DescriptorPublicKey> {
        self.spend_descriptor.clone()
    }
}
