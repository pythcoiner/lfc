use std::{
    io::{self, Write},
    process::{self},
    str::FromStr,
};

use crate::{channel_state::ChannelState, parse_tx, round::Rounds, FEE, MAX_DERIV};
use bip39::Mnemonic;
use clap::Subcommand;
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
    println!("{}", txt);
    std::io::stdout().flush().unwrap();
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
    // println!("conf: path={:?}", args.path);
    assert!(matches!(args.command, Command::Conf { .. }));
    let index = input(&format!(
        "Select a derivation index for the wallet: 0-{}",
        MAX_DERIV
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
        "Enter the mnemonic for your master wallet \
        (12 words), if the input is empty a mnemonic phrase will be automatically generated:",
    )
    .trim()
    .to_string();
    let master_mnemonic = if mnemo.is_empty() {
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
        master_mnemonic,
        spend_mnemonic,
        amount,
        delay,
        account: index,
        network,
        rounds: Rounds::new(),
        path: args.path,
    };
    conf.to_file().unwrap();

    println!("Configuration file saved at {:?}!", conf.path);
}

pub fn create(mut args: Args) {
    assert!(matches!(args.command, Command::Create));
    let mut state = args.state.take().unwrap();

    let funding_addr = state.funding_address();

    println!("Address to fund the contract: {}", funding_addr);

    let mut raw_tx = input("Enter raw tx that fund the contract:");
    raw_tx = raw_tx.trim().into();

    let tx0: Result<Transaction, _> = consensus::encode::deserialize_hex(&raw_tx);
    let funding_tx = match tx0 {
        Ok(tx) => tx,
        Err(e) => {
            eprintln!("Fail to parse transaction: \n {} \n {}", raw_tx, e);
            std::process::exit(1);
        }
    };

    if let Err(e) = state.generate_rounds(funding_tx, FEE, true) {
        println!("{e}");
        std::process::exit(1);
    }

    println!("Successfully generated {} rounds!", state.rounds.count());

    state.to_file().unwrap();
}

pub fn sign(mut args: Args) {
    assert!(matches!(args.command, Command::Sign));
    let mut state = args.state.take().unwrap();

    if state.sign_rounds(true).is_ok() {
        println!("Successfully signed {} rounds!", state.rounds.count());
        state.to_file().unwrap();
    } else {
        process::exit(1)
    }
}

pub fn unlock(mut args: Args) {
    assert!(matches!(args.command, Command::Unlock));
    let mut state = args.state.take().unwrap();

    fn on_unlock_tx(tx: Transaction, height: Option<u64>, index: usize, next: bool) {
        let round = if next { "next" } else { "current" };
        let tx = consensus::encode::serialize_hex(&tx);
        if let Some(height) = height {
            println!(
                    "Round {index} => Broadcast this transaction to unlock the {round} round after block height {height}: \n{tx}",
                );
        } else {
            println!(
                "Round {index} => Broadcast this transaction to unlock the {round} round: \n{tx}",
            );
        }
    }

    match state.unlock_next() {
        Ok((tx, height, index, next)) => {
            on_unlock_tx(tx, height, index, next);
        }
        Err(e) => println!("{e}"),
    }
}

pub fn register(mut args: Args, tx: Transaction, height: u64) {
    assert!(matches!(args.command, Command::Register { .. }));
    let mut state = args.state.take().unwrap();
    let registered = state.register(tx, height);
    println!("{registered}");

    state.to_file().unwrap();
}

pub fn status(args: Args) {
    assert!(matches!(args.command, Command::Status));
    let state = args.state.unwrap();

    let status = serde_json::to_string_pretty(&state).unwrap();
    eprintln!("{status}")
}

pub fn spend(args: Args) {
    assert!(matches!(args.command, Command::Spend { .. }));
    let state = args.state.unwrap();
    if let Command::Spend { amount, address } = args.command {
        let amount = Amount::from_btc(amount).unwrap().to_sat();
        match state.create_spend(address, amount) {
            Ok((tx, amount, addr)) => {
                let tx = consensus::encode::serialize_hex(&tx);
                println!(
                    "Broadcast this transaction for spend {} to {}: \n{}",
                    amount, addr, tx
                );
            }
            Err(e) => {
                println!("{e}");
                process::exit(1)
            }
        }
    } else {
        unreachable!()
    }
}

pub fn spendable(args: Args) {
    assert!(matches!(args.command, Command::Spendable));
    let state = args.state.unwrap();
    let spendable_coins = state.rounds.spendable_coins();
    if spendable_coins.is_empty() {
        println!("No spendable coins!");
        return;
    }

    let spendable_amt = state.rounds.spendable_amount();
    println!("Total spendable: {spendable_amt}");
    for c in spendable_coins {
        println!("{}: {}", c.outpoint, c.prevout.value);
    }
}
