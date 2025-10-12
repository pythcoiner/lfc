use std::{io, str::FromStr};

use bip39::Mnemonic;
use clap::Subcommand;
use lfc::{
    channel::Channel,
    channel_state::ChannelState,
    parse_tx,
    round::{Round, Rounds},
    FEE, MAX_DERIV,
};
use miniscript::bitcoin::{
    address::NetworkUnchecked, consensus, Address, Amount, Network, Transaction,
};

use super::Args;

#[derive(Subcommand, PartialEq, Debug)]
pub enum Command {
    /// Display the status of the wallet
    Status,
    /// Generate a new wallet config
    Conf {
        /// Bitcoin network: bitcoin/testnet/signet/regtest
        #[arg(global = true, short, long, default_value_t = Network::Regtest)]
        network: Network,
    },
    /// Create a chain of transaction
    Create,
    /// Sign all presigned PSBTs
    Sign,
    /// Start an unlock round
    Unlock,
    /// Register a confirmed transaction
    Register {
        /// Block height where the transaction have been included in a block
        height: u64,
        /// Transaction in hex format
        #[arg( value_parser = parse_tx)]
        transaction: Transaction,
    },
    /// Broadcast a lock transaction if available
    Lock,
    /// Relock an available coin
    Relock,
    /// Spend from an available coin
    #[command(alias = "send")]
    Spend {
        /// Amount to send in BTC
        amount: f64,
        /// Recipient address
        address: Address<NetworkUnchecked>,
    },
    /// List spendable coins
    #[command(alias = "sendable")]
    Spendable,
    /// Delete the wallet
    Del,
}

pub fn input(txt: &str) -> String {
    eprintln!("{}", txt);
    let mut name = String::new();
    _ = io::stdin().read_line(&mut name).unwrap();
    name = name.trim().to_string();
    name
}

pub fn del_wallet(args: Args) {
    assert!(matches!(args.command, Command::Del));
    let hint = format!("Are you sure to delete wallet {}?", args.wallet);
    let inp = input(&hint);
    if ["y".to_string(), "yes".to_string()].contains(&inp.to_lowercase()) {
        std::fs::remove_file(args.path).unwrap();
    }
}

pub fn conf(args: Args) {
    assert!(matches!(args.command, Command::Conf { .. }));
    let index = input(&format!(
        "Select a derivation index for the wallet: 0-{}",
        lfc::MAX_DERIV
    ));
    let index = match index.parse::<u32>() {
        Ok(i) => {
            if i < MAX_DERIV {
                Some(i)
            } else {
                None
            }
        }
        Err(_) => None,
    }
    .unwrap();

    let mnemo = input(
        "Enter the mnemonic for your covenant wallet \
        (12 words), if the input is empty a mnemonic phrase will be automatically generated:",
    )
    .trim()
    .to_string();
    let cov_mnemonic = if mnemo.is_empty() {
        Mnemonic::generate(12).unwrap().to_string()
    } else {
        Mnemonic::from_str(&mnemo).unwrap().to_string()
    };

    let mnemo = input(
        "Enter the mnemonic for your spending wallet \
        (12 words), if the input is empty a mnemonic phrase will be automatically generated:",
    )
    .trim()
    .to_string();
    let spend_mnemonic = if mnemo.is_empty() {
        Mnemonic::generate(12).unwrap().to_string()
    } else {
        Mnemonic::from_str(&mnemo).unwrap().to_string()
    };

    let amount = input("Enter the max amount allowed to spend at each round: (BTC)");
    let amount = f64::from_str(&amount).unwrap();
    let amount = Amount::from_btc(amount).unwrap().to_sat();

    let delay = input("Enter the number of block minimum between 2 rounds:");
    let delay = delay.parse::<u16>().unwrap();

    let network = if let Command::Conf { network } = args.command {
        network
    } else {
        unreachable!()
    };

    let conf = ChannelState {
        cov_mnemonic,
        spend_mnemonic,
        amount,
        delay,
        account: index,
        network,
        rounds: Rounds::new(),
        path: args.path,
    };
    conf.to_file().unwrap();
}

pub fn create(mut args: Args) {
    assert!(matches!(args.command, Command::Create));
    let mut state = args.state.take().unwrap();

    let channel = Channel::from_state(&state);
    let funding_addr = channel.cov_addr(0);

    eprintln!("Address to fund the contract: {}", funding_addr);

    let mut raw_tx = input("Enter raw tx that fund the contract:");
    raw_tx = raw_tx.trim().into();

    let tx0: Result<Transaction, _> = consensus::encode::deserialize_hex(&raw_tx);
    let tx0 = match tx0 {
        Ok(tx) => tx,
        Err(e) => {
            eprintln!("Fail to parse transaction: \n {} \n {}", raw_tx, e);
            std::process::exit(1);
        }
    };

    if tx0.output[0].script_pubkey != funding_addr.script_pubkey() {
        eprintln!("The first output of the tx must fund the funding address!");
        std::process::exit(1);
    }

    let amount = tx0.output[0].value.to_sat();

    if amount == 0 {
        eprintln!("Amount of funding input must be > 0");
        std::process::exit(1);
    }

    let mut txs = Vec::new();
    let mut previous_tx = tx0;
    let mut previous_amount = amount;
    let mut index = 1;

    loop {
        let (spend, relock) = if previous_amount > state.amount {
            let mut relock = previous_amount.saturating_sub(state.amount);
            if relock <= FEE {
                relock = 0;
            }
            let spend = if relock == 0 {
                previous_amount.saturating_sub(FEE)
            } else {
                state.amount
            };
            (spend, relock)
        } else {
            (previous_amount - FEE, 0)
        };
        let psbt = channel.craft_tx(previous_tx.clone(), index, spend, relock);
        previous_tx = psbt.unsigned_tx.clone();
        previous_amount = previous_tx.output[0].value.to_sat();
        index += 1;

        txs.push(psbt);

        if relock < (2 * FEE) {
            break;
        }
    }

    let mut index = 0;
    #[allow(clippy::explicit_counter_loop)]
    for psbt in txs {
        index += 1;
        let round = Round::new(psbt, index);
        state.rounds.push(round);
    }

    state.to_file().unwrap();
}

pub fn sign(mut args: Args) {
    assert!(matches!(args.command, Command::Sign));
    let mut state = args.state.take().unwrap();
    assert!(!state.rounds.is_empty());

    let _channel = Channel::from_state(&state);

    let s = state.clone();
    for _round in state.rounds.as_mut_vec() {
        let _s = s.clone();
        // FIXME::
        // round.sign(|psbt| {
        //     channel.presign_psbt(psbt, state);
        // });
    }

    state.to_file().unwrap();
}

pub fn unlock(mut args: Args) {
    assert!(matches!(args.command, Command::Unlock));
    let mut state = args.state.take().unwrap();
    let rounds = &mut state.rounds;

    let index = rounds.current_round_index();

    let current = rounds.at(index).unwrap();
    match current.unlock() {
        Some(tx) => {
            println!(
                    "Broadcast this transaction tu unlock the current round after block height {}: \n{}",
                    current.unlock_after().unwrap(),
                    serde_json::to_string(&tx).unwrap()
                );
        }
        None => {
            if rounds.at(index + 1).is_none() {
                println!("No more round to unlock!");
            } else {
                println!("Fail to get unlock transaction!");
            }
        }
    }
}

pub fn register(mut args: Args, tx: Transaction, height: u64) {
    assert!(matches!(args.command, Command::Register { .. }));
    let mut state = args.state.take().unwrap();
    let delay = state.delay() as u64;
    let rounds = &mut state.rounds;

    let (unlock, spend) = rounds.register(tx, height, delay);
    if unlock {
        println!("Unlock registered!")
    } else if spend {
        println!("Spend registered!")
    }

    state.to_file().unwrap();
}

pub fn status(args: Args) {
    assert!(matches!(args.command, Command::Status));
    let state = args.state.unwrap();

    let status = serde_json::to_string_pretty(&state).unwrap();
    eprintln!("{status}")
}
