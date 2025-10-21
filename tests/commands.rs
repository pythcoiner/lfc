use std::{path::PathBuf, str::FromStr};

use corepc_node::{Client, Conf};
use lfc::{
    channel::Channel,
    channel_state::{ChannelState, Registered},
};
use miniscript::bitcoin::{Amount, BlockHash, Network, Txid};

fn get_height(client: &mut Client) -> u64 {
    client.get_block_count().unwrap().0
}

fn generate_blocks(client: &mut Client, blocks: usize) {
    let height = get_height(client);
    for _ in 0..blocks {
        let addr = client.new_address().unwrap();
        client.generate_to_address(1, &addr).unwrap();
    }
    let new_height = get_height(client);
    assert_eq!(new_height, height + blocks as u64);
}

fn get_tx_height(client: &mut Client, txid: Txid) -> Option<u64> {
    let hash = client.get_raw_transaction_verbose(txid).unwrap().block_hash;
    hash.map(|h| {
        let bh = BlockHash::from_str(&h).unwrap();
        client.get_block(bh).unwrap().bip34_block_height().unwrap()
    })
}

#[test]
fn test_lib_flow() {
    const AMOUNT: u64 = 35_000_000;
    const DELAY: u16 = 100;
    let mut conf = Conf::default();
    conf.args.push("-txindex");
    let mut node = corepc_node::Node::from_downloaded_with_conf(&conf).unwrap();
    let bitcoind = &mut node.client;

    generate_blocks(bitcoind, 105);

    let height = bitcoind.get_blockchain_info().unwrap().blocks;
    assert_eq!(height, 105);

    // Create the wallet
    let master_mnemonic =
        "task glad violin popular angry arrange assume debate welcome earth crunch side"
            .to_string();
    let spend_mnemonic =
        "ceiling blue sing poverty bean yellow fat basket merge behave crystal tiny".to_string();

    let mut state = ChannelState {
        master_mnemonic,
        spend_mnemonic,
        amount: AMOUNT,
        delay: DELAY,
        account: 0,
        network: Network::Regtest,
        rounds: Default::default(),
        path: PathBuf::new(),
    };
    let channel = Channel::from_state(&state);

    // Fund the contract
    let funding_addr = state.funding_address();

    let res = bitcoind
        .send_to_address(&funding_addr, Amount::from_sat(100_000_000))
        .unwrap();
    let txid = res.txid().unwrap();
    let tx = bitcoind
        .get_raw_transaction(txid)
        .unwrap()
        .transaction()
        .unwrap();

    generate_blocks(bitcoind, 5);

    let funding_height = get_tx_height(bitcoind, txid).unwrap();
    assert_eq!(funding_height, 106);

    assert!(state.transactions().is_empty());
    // Generate rounds
    state.generate_rounds(tx, 300, false).unwrap();
    assert_eq!(state.rounds.count(), 3);
    assert_eq!(state.transactions().len(), 1);

    state.sign_rounds(false).unwrap();
    assert_eq!(state.transactions().len(), 1);

    // Unlock the first round
    let (unlock1_tx, height, _, _) = state.unlock_next().unwrap();
    // the first unlock is not timelocked
    assert_eq!(height, None);
    // output 1 unlock AMOUNT BTC
    assert_eq!(unlock1_tx.output[1].value, Amount::from_sat(AMOUNT));
    // to spend address at index 1
    assert_eq!(
        unlock1_tx.output[1].script_pubkey,
        channel.spend_addr(1).script_pubkey()
    );

    let unlock1_txid = bitcoind
        .send_raw_transaction(&unlock1_tx)
        .unwrap()
        .txid()
        .unwrap();

    let unlock1_height = get_tx_height(bitcoind, unlock1_txid);
    assert_eq!(unlock1_height, None);

    let height = get_height(bitcoind);

    generate_blocks(bitcoind, 3);
    let unlock1_height = height + 1;
    let tx_height = get_tx_height(bitcoind, unlock1_txid).unwrap();
    assert_eq!(tx_height, unlock1_height);

    assert_eq!(state.transactions().len(), 1);
    let registered = state.register(unlock1_tx, unlock1_height);
    assert_eq!(registered, Registered::Unlock);
    assert_eq!(state.transactions().len(), 2);

    // Partial spend
    let spend_addr = bitcoind.new_address().unwrap().as_unchecked().clone();
    let (spend_tx, am, ad) = state.create_spend(spend_addr.clone(), 10_000_000).unwrap();
    assert_eq!(am, Amount::from_sat(10_000_000));
    assert_eq!(ad, spend_addr.assume_checked());

    let txid = bitcoind
        .send_raw_transaction(&spend_tx)
        .unwrap()
        .txid()
        .unwrap();
    let height = get_height(bitcoind);
    generate_blocks(bitcoind, 3);
    let spend1_height = get_tx_height(bitcoind, txid).unwrap();
    assert_eq!(spend1_height, height + 1);

    assert_eq!(state.transactions().len(), 2);
    let registered = state.register(spend_tx, spend1_height);
    assert_eq!(registered, Registered::Spend);
    assert_eq!(state.transactions().len(), 3);

    let spendable = state.spendable_amount();
    assert_eq!(spendable, Amount::from_sat(24_999_850));

    // generate blocks up to 2 height prior the next round unlock
    let height = get_height(bitcoind);
    let blocks = unlock1_height + DELAY as u64 - height - 2;
    generate_blocks(bitcoind, blocks as usize);

    // the next round unlock cannot yet been broadcast
    let actual_height = get_height(bitcoind);
    let (tx, height, ..) = state.unlock_next().unwrap();
    assert_eq!(height.unwrap(), actual_height + 2);
    let err = bitcoind.send_raw_transaction(&tx).unwrap_err().to_string();
    assert_eq!(err, "JSON-RPC error: RPC error response: RpcError { code: -26, message: \"non-BIP68-final\", data: None }");

    generate_blocks(bitcoind, 1);

    // now it can be brodcasted in order to be mined in the next block
    let txid = bitcoind.send_raw_transaction(&tx).unwrap().txid().unwrap();

    generate_blocks(bitcoind, 3);

    let expected_height = unlock1_height + DELAY as u64;
    let unlock2_height = get_tx_height(bitcoind, txid).unwrap();
    assert_eq!(expected_height, unlock2_height);

    assert_eq!(state.transactions().len(), 3);
    let registered = state.register(tx, unlock2_height);
    assert_eq!(registered, Registered::Unlock);
    assert_eq!(state.transactions().len(), 4);

    let new_spendable = state.spendable_amount();
    assert_eq!(new_spendable, spendable + Amount::from_sat(AMOUNT));

    // Second spend
    let spend_addr = bitcoind.new_address().unwrap().as_unchecked().clone();
    let (spend2_tx, am, ad) = state.create_spend(spend_addr.clone(), 20_000_000).unwrap();
    assert_eq!(am, Amount::from_sat(20_000_000));
    assert_eq!(ad, spend_addr.assume_checked());
    // we spend the 2 available coins
    assert_eq!(spend2_tx.input.len(), 2);

    let txid = bitcoind
        .send_raw_transaction(&spend2_tx)
        .unwrap()
        .txid()
        .unwrap();
    generate_blocks(bitcoind, 1);
    let spend2_height = get_tx_height(bitcoind, txid).unwrap();

    assert_eq!(state.transactions().len(), 4);
    let registered = state.register(spend2_tx, spend2_height);
    assert_eq!(registered, Registered::Spend);
    assert_eq!(state.transactions().len(), 5);

    let spendable = state.spendable_amount();
    assert_eq!(spendable, Amount::from_sat(39_999_700));
}
