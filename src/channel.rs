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
    psbt::{PsbtInputExt, PsbtOutputExt},
    Descriptor, DescriptorPublicKey,
};

const SUB_ACCOUNT: u32 = 0;

#[macro_export]
macro_rules! index_key {
    () => {
        ProprietaryKey {
            prefix: b"lfc".to_vec(),
            subtype: 0x01,
            key: b"index".to_vec(),
        }
    };
}

use crate::{channel_state::ChannelState, SECP};

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
    cov: DescriptorPublicKey,
    #[allow(unused)]
    spend: DescriptorPublicKey,
    timelock: u16,
    network: Network,
    cov_descriptor: Descriptor<DescriptorPublicKey>,
    spend_descriptor: Descriptor<DescriptorPublicKey>,
}

impl Channel {
    pub fn from_state(state: &ChannelState) -> Self {
        let cov = state.cov_xpub(SUB_ACCOUNT).unwrap();
        let spend = state.spend_xpub(SUB_ACCOUNT).unwrap();
        let timelock = state.delay();
        let network = state.network;

        let cov_descriptor = Descriptor::Wpkh(Wpkh::new(cov.clone()).unwrap());

        println!("cov_descriptor: \n \n{} \n \n", cov_descriptor);

        let spend_descriptor = Descriptor::Wpkh(Wpkh::new(spend.clone()).unwrap());

        println!("spend_descriptor: \n \n{} \n \n", spend_descriptor);

        Self {
            cov,
            spend,
            timelock,
            network,
            cov_descriptor,
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

    pub fn cov_addr(&self, index: u32) -> Address {
        self.cov_descriptor
            .at_derivation_index(index)
            .expect("must not fail")
            .address(self.network)
            .expect("must not fail")
    }

    pub fn craft_tx(&self, previous_tx: Transaction, index: u32, spend: u64, relock: u64) -> Psbt {
        println!(
            "craft_tx(index: {}, spend: {}, relock: {})",
            index, spend, relock,
        );
        let spend = Amount::from_sat(spend);
        let relock = Amount::from_sat(relock);
        let relock_addr = self.cov_addr(index);
        let relock_out = TxOut {
            value: relock,
            script_pubkey: relock_addr.into(),
        };
        let spend_addr = self.spend_addr(index);
        let spend_out = TxOut {
            value: spend,
            script_pubkey: spend_addr.into(),
        };

        let outpoint = OutPoint {
            txid: previous_tx.compute_txid(),
            vout: 0,
        };
        let sequence = if index == 1 { 0 } else { self.timelock };
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

        // the previous tx address must have been generated at index-1
        assert!(self
            .cov_addr(index - 1)
            .matches_script_pubkey(&previous_tx.output[0].script_pubkey));
        let spend_descriptor = self.cov_descriptor.at_derivation_index(index - 1).unwrap();

        psbt_input
            .update_with_descriptor_unchecked(&spend_descriptor)
            .unwrap();

        psbt_input.witness_utxo = Some(previous_tx.output[0].clone());

        let psbt_inputs = vec![psbt_input];

        let mut psbt_relock = Output::default();
        let out_descriptor = self.cov_descriptor.at_derivation_index(index).unwrap();
        psbt_relock
            .update_with_descriptor_unchecked(&out_descriptor)
            .unwrap();
        let psbt_spend = Output::default();

        let psbt_outputs = if relock != Amount::ZERO {
            vec![psbt_relock, psbt_spend]
        } else {
            vec![psbt_spend]
        };

        // store the index in the proprietary map
        let mut proprietary = BTreeMap::new();
        proprietary.insert(index_key!(), index.to_le_bytes().to_vec());

        Psbt {
            unsigned_tx: tx,
            version: 0,
            xpub: BTreeMap::new(),
            proprietary,
            unknown: BTreeMap::new(),
            inputs: psbt_inputs,
            outputs: psbt_outputs,
        }
    }

    pub fn presign_psbt(&self, psbt: &mut Psbt, state: &ChannelState) {
        // index is stored in the proprietary map
        let raw_index: [u8; 4] = psbt
            .proprietary
            .get(&index_key!())
            .unwrap()
            .to_vec()
            .try_into()
            .unwrap();
        let index = u32::from_le_bytes(raw_index);

        assert_eq!(psbt.inputs.len(), 1);
        // TODO: verify outputs go to the right descriptors

        let xpriv = state.cov_xpriv_at(SUB_ACCOUNT, index);
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
        let witness = Witness::p2wpkh(&signature, &pk);

        psbt.inputs[0].final_script_witness = Some(witness);
    }
}
