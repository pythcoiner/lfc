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

    pub fn craft_tx(&self, previous_tx: Transaction, index: u32, spend: u64, relock: u64) -> Psbt {
        // TODO: do not hardcode fees
        const DUST: u64 = 500;
        let vbytes = 150;
        #[allow(clippy::identity_op)]
        let fees = vbytes * 1; /* sats */
        println!("Generate transaction for round {index}, spending {spend} and relocking {relock}");

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

        let relock = if relock < DUST { 0 } else { relock };

        let spend = Amount::from_sat(spend);
        let relock = Amount::from_sat(relock);
        let relock_addr = self.master_addr(index);
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
            .master_addr(index - 1)
            .matches_script_pubkey(&previous_tx.output[0].script_pubkey));
        let input_descriptor = self
            .master_descriptor
            .at_derivation_index(index - 1)
            .unwrap();

        psbt_input.witness_utxo = Some(previous_tx.output[0].clone());

        let psbt_inputs = vec![psbt_input];

        let relock_decriptor = self.master_descriptor.at_derivation_index(index).unwrap();
        let spend_descriptor = self.spend_descriptor.at_derivation_index(index).unwrap();
        let output_relock = Output::default();
        let output_spend = Output::default();

        let psbt_outputs = if relock != Amount::ZERO {
            vec![output_relock, output_spend]
        } else {
            vec![output_spend]
        };

        // store the index in the proprietary map
        let mut proprietary = BTreeMap::new();
        proprietary.insert(index_key!(), index.to_le_bytes().to_vec());

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

    pub fn presign_psbt(&self, psbt: &mut Psbt, state: &ChannelState) {
        // index is stored in the proprietary map
        let raw_index: [u8; 4] = psbt
            .proprietary
            .get(&index_key!())
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
}
