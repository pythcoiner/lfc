use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    process::{self, Output},
    thread::{self, JoinHandle},
    time::{self, Duration},
};

use lfc::channel_state::ChannelState;
use os_pipe::{PipeReader, PipeWriter};
use temp_dir::TempDir;

fn lfc(
    args: Vec<String>,
    tmp_dir: Option<TempDir>,
) -> (
    BufWriter<PipeWriter>,
    BufReader<PipeReader>,
    JoinHandle<Output>,
    TempDir,
) {
    // let tmp_dir = temp_dir::TempDir::new().unwrap();
    let tmp_dir = if let Some(tmp_dir) = tmp_dir {
        tmp_dir
    } else {
        temp_dir::TempDir::new().unwrap()
    };
    let (stdout, sender) = os_pipe::pipe().unwrap();
    let (receiver, stdin) = os_pipe::pipe().unwrap();
    let datadir = tmp_dir.path().to_str().unwrap().to_string();

    let cmd = thread::spawn(move || {
        let mut cmd = process::Command::new(env!("CARGO_BIN_EXE_lfc"));
        let mut cmd = cmd.arg("--datadir").arg(datadir);
        for arg in args {
            cmd = cmd.arg(arg);
        }
        let child = cmd
            .stdin(receiver)
            .stdout(sender.try_clone().unwrap())
            .stderr(sender)
            .spawn()
            .unwrap();
        child.wait_with_output().unwrap()
    });
    (BufWriter::new(stdin), BufReader::new(stdout), cmd, tmp_dir)
}

fn print_stdout(stdout: &mut BufReader<PipeReader>, prefix: &str) {
    let mut str = String::new();
    if let Ok(l) = stdout.read_line(&mut str) {
        if l > 0 {
            println!("{prefix}{str}");
        }
    }
}

fn output(mut stdout: BufReader<PipeReader>) -> Option<(BufReader<PipeReader>, Option<String>)> {
    let timeout = time::SystemTime::now()
        .checked_add(Duration::from_secs(5))
        .unwrap();
    let handle = std::thread::spawn(move || {
        let mut out = String::new();
        let o = if let Ok(chars) = stdout.read_line(&mut out) {
            (chars > 0).then_some(out)
        } else {
            None
        };
        (stdout, o)
    });
    while time::SystemTime::now() < timeout {
        if handle.is_finished() {
            return handle.join().ok();
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

fn output_contains(text: &str, stdout: BufReader<PipeReader>) -> BufReader<PipeReader> {
    let (stdout, prompt) = output(stdout).unwrap();
    let prompt = prompt.unwrap_or_default();
    if !prompt.contains(text) {
        panic!("[{prompt}] does not contains [{text}]!");
    }
    println!("{prompt}");
    stdout
}

fn send_stdin(text: &str, stdin: &mut BufWriter<PipeWriter>) {
    let text = if text.ends_with("\n") {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    print!("{text}");
    stdin.write_all(text.as_bytes()).unwrap();
    stdin.flush().unwrap();
}

fn init_conf(
    index: u32,
    master_mnemonic: Option<String>,
    spend_mnemonic: Option<String>,
    amount: f64,
    delay: u64,
) -> TempDir {
    let (mut stdin, mut stdout, _handle, tmp_dir) = lfc(vec!["conf".to_string()], None);

    print_stdout(&mut stdout, "");

    send_stdin(&format!("{index}"), &mut stdin);
    print_stdout(&mut stdout, "");

    let master_mnemonic = master_mnemonic.unwrap_or_default().to_string();
    let spend_mnemonic = spend_mnemonic.unwrap_or_default().to_string();
    send_stdin(&master_mnemonic, &mut stdin);
    print_stdout(&mut stdout, "");

    send_stdin(&spend_mnemonic, &mut stdin);
    print_stdout(&mut stdout, "");

    send_stdin(&format!("{amount}"), &mut stdin);
    print_stdout(&mut stdout, "");

    send_stdin(&format!("{delay}"), &mut stdin);

    let _stdout = output_contains("Configuration file saved", stdout);
    tmp_dir
}

#[allow(unused)]
fn dump(timeout: u64, stdout: &mut BufReader<PipeReader>) {
    let stop = time::SystemTime::now()
        .checked_add(Duration::from_secs(timeout))
        .unwrap();
    while time::SystemTime::now() < stop {
        print_stdout(stdout, "- ");
    }
}

const WAIT: u64 = 20;

#[test]
fn test_cli_conf() {
    let (mut stdin, mut stdout, _handle, tmp_dir) = lfc(vec!["conf".to_string()], None);

    stdout = output_contains("Select a derivation index for the wallet", stdout);
    send_stdin("0", &mut stdin);

    stdout = output_contains("Enter the mnemonic for your master wallet", stdout);
    send_stdin("", &mut stdin);

    stdout = output_contains("Enter the mnemonic for your spending wallet", stdout);
    send_stdin("", &mut stdin);

    stdout = output_contains(
        "Enter the max amount allowed to spend at each round",
        stdout,
    );
    send_stdin("0.01", &mut stdin);

    stdout = output_contains("Enter the number of block minimum between 2 rounds", stdout);
    send_stdin("100", &mut stdin);

    let _stdout = output_contains("Configuration file saved", stdout);

    let mut dir = tmp_dir.path().to_path_buf();
    dir.push("lfc.conf");
    assert!(dir.exists());

    let state = ChannelState::from_file(dir.to_str().unwrap()).unwrap();
    assert!(!state.master_mnemonic.is_empty());
    assert!(!state.spend_mnemonic.is_empty());
    assert_eq!(state.amount, 1_000_000);
    assert_eq!(state.delay, 100);
}

#[test]
fn test_init_conf() {
    let _ = init_conf(0, None, None, 0.1, 100);
}

#[test]
#[should_panic]
fn test_conf_wrong_cold_mnemonic() {
    let _ = init_conf(0, Some("wrong mnemo".to_string()), None, 0.1, 100);
}

#[test]
#[should_panic]
fn test_conf_wrong_spend_mnemonic() {
    let _ = init_conf(0, None, Some("wrong".to_string()), 0.1, 100);
}

#[test]
#[should_panic]
fn test_conf_wrong_index() {
    let _ = init_conf(u32::MAX, None, None, 0.1, 100);
}

#[test]
fn test_del() {
    let datadir = init_conf(0, None, None, 0.1, 100);

    let mut dir = datadir.path().to_path_buf();
    dir.push("lfc.conf");
    assert!(dir.exists());
    assert!(dir.is_file());

    std::thread::sleep(Duration::from_secs(2));

    let (mut stdin, stdout, _, datadir) = lfc(vec!["del".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));
    let _stdout = output_contains("Are you sure to delete wallet", stdout);
    send_stdin("n", &mut stdin);
    std::thread::sleep(Duration::from_millis(WAIT));
    assert!(dir.exists());

    // NOTE: _tmp must note be drop else we have a false positive of deletion
    let (mut stdin, stdout, _, _tmp) = lfc(vec!["del".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));
    let _stdout = output_contains("Are you sure to delete wallet", stdout);
    send_stdin("y", &mut stdin);

    std::thread::sleep(Duration::from_millis(WAIT));
    assert!(!dir.exists());
}

#[test]
fn test_cli_flow() {
    // Create
    let master = "task glad violin popular angry arrange assume debate welcome earth crunch side"
        .to_string();
    let spend =
        "ceiling blue sing poverty bean yellow fat basket merge behave crystal tiny".to_string();
    let datadir = init_conf(0, Some(master), Some(spend), 0.35, 100);

    let mut dir = datadir.path().to_path_buf();
    dir.push("lfc.conf");

    std::thread::sleep(Duration::from_secs(1));

    let (mut stdin, mut stdout, _, datadir) = lfc(vec!["create".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    stdout = output_contains(
        "Address to fund the contract: bcrt1q70rqqql720wv2gr0q8u3rleqqu7h05e7z88mvg",
        stdout,
    );
    stdout = output_contains("Enter raw tx that fund the contract", stdout);

    let funding_tx = "02000000000101acdd26f0527f7cb38893c8b43fcfa763608eb63a75f9c963ad91f24474627f2e0100000000fdffffff0200e1f50500000000160014f3c60003fe53dcc5206f01f911ff20073d77d33ebc06af2f000000002200207bb9b19ab798250a079627b6e051268cdb325f42fb185e0077a9278dd390af3c02473044022036a7cf0968efd9f929347a50b9e4c8ce2ca4604cbc76a5ecc675636b7fda6f8e02204b6d5302d613327733b72d34168fdbb4cb672ca77a0c7346f6ada33eac6c001c01442102dadf0cd38b4c8f60e210d7fc1e19c1e9bf4618ecd53b404bb65d63f1b9b9adb3ac736476a914bc3e3cb7abd61c960b47231324b7e19896e4355f88ad0374cd00b268dc030000";
    send_stdin(funding_tx, &mut stdin);

    let mut buf = String::new();
    for _ in 0..3 {
        let _ = stdout.read_line(&mut buf);
    }

    let _stdout = output_contains("Successfully generated 3 rounds!", stdout);

    // Sign
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["sign".to_string()], Some(datadir.clone()));
    std::thread::sleep(Duration::from_millis(WAIT));

    let mut buf = String::new();
    for _ in 0..3 {
        let _ = stdout.read_line(&mut buf);
    }

    let _stdout = output_contains("Successfully signed 3 rounds!", stdout);

    // Unlock round 0
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    stdout = output_contains(
        "Broadcast this transaction to unlock the current round",
        stdout,
    );

    let unlock_tx = "020000000001010afe135a00314cb07f00bc168c4fb51781e76417363a39eae4097f4b8c55d2a300000000000000000002aad1df0300000000160014c0585c8ba63c22a2ed2592a5d35208cbde64643fc00e1602000000001600143e8afa59f9e340af7a5018f399e0e00d05c5426202473044022023206a4085d1c23b9119fc05da16b219352fc70f4809001cee2e17e6770cf5ba02200ed172a1624809066ed1f0e5a5d69420f1e8893095178d4b1acc0dd9229f5c47012103407c3190500a371290d5654ea51d986afcf405fc10bd0131b8dfb800e61f3ee600000000";
    let _stdout = output_contains(unlock_tx, stdout);

    // Register
    let (_stdin, stdout, _, datadir) = lfc(
        vec![
            "register".to_string(),
            "1000".to_string(),    // height
            unlock_tx.to_string(), // tx
        ],
        Some(datadir),
    );
    std::thread::sleep(Duration::from_millis(WAIT));

    let _stdout = output_contains("Unlock registered", stdout);

    // Unlock round 1
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    stdout = output_contains(
        "Broadcast this transaction to unlock the next round after block height ",
        stdout,
    );

    let unlock_tx_2 = "02000000000101405a85a3ba4635ee2cc8384627ae3e98867726de69be8b2d46fac25da0995f180000000000640000000254c2c901000000001600149a115404fedcd2525639c7ce8e6bcab96d062a74c00e1602000000001600145452c0a91064bf144f74fba548bd31c98909b0df0247304402201c106aa5e90f6a5c683659f434ce3a938c0175f9abc7a5605b59499d4e0f025d022037b49fb2656ba32086219578c82f9a1b1716e102ec4fc77d7b12ca32e7e68bb6012102daf01b75eee0d904ab3d05c903976eaaa5373191e552ae73312916c719f1918900000000";
    let _stdout = output_contains(unlock_tx_2, stdout);

    // Register
    let (_stdin, stdout, _, datadir) = lfc(
        vec![
            "register".to_string(),
            "1101".to_string(),      // height
            unlock_tx_2.to_string(), // tx
        ],
        Some(datadir),
    );
    std::thread::sleep(Duration::from_millis(WAIT));

    let _stdout = output_contains("Unlock registered", stdout);

    // Unlock round 2
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    stdout = output_contains(
        "Broadcast this transaction to unlock the next round after block height ",
        stdout,
    );

    let unlock_tx_3 = "02000000000101081060af91aa4cbbbb87f6d0945b56efed34454bd273b4ff570b430cdde2bd6d0000000000640000000166bfc90100000000160014195dc81d4622339e0676721705ad05dc719fa9830247304402205ad0f0c81eaedacbdf4c47027f78cd76cfadc383259af21987a2c2ee17c43a360220544a6c4fcd43dedd5e58ad916ff59e19a4ddfbdff653a41578c4ce6e3e428cfa012102b7be4e1593ea444428da822b31da0581477ced632a38139d02e46dc690f8dde200000000";
    let _stdout = output_contains(unlock_tx_3, stdout);

    // Register
    let (_stdin, stdout, _, datadir) = lfc(
        vec![
            "register".to_string(),
            "1202".to_string(),      // height
            unlock_tx_3.to_string(), // tx
        ],
        Some(datadir),
    );
    std::thread::sleep(Duration::from_millis(WAIT));

    let _stdout = output_contains("Unlock registered", stdout);

    // Nothing to unlock
    let (_stdin, stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    let _stdout = output_contains("No more round to unlock", stdout);

    // List spendable
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["spendable".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    // 3 coins are spendable
    stdout = output_contains("0.99998950", stdout);
    stdout = output_contains(
        "6dbde2dd0c430b57ffb473d24b4534edef565b94d0f687bbbb4caa91af601008:1: 0.35000000 ",
        stdout,
    );
    stdout = output_contains(
        "64de0152b67eb304b97183c453491f06d17343a47c7c3c122368f1f8f12eeb33:0: 0.29998950 ",
        stdout,
    );
    let _stdout = output_contains(
        "185f99a05dc2fa462d8bbe69de267786983eae274638c82cee3546baa3855a40:1: 0.35000000 ",
        stdout,
    );

    // Spend 0.1 BTC
    let (_stdin, mut stdout, _, datadir) = lfc(
        vec![
            "spend".to_string(),
            "0.1".to_string(),
            "bcrt1q6s79sgusgxzn25rve8nxfcu0fkddedg24hlavh".to_string(),
        ],
        Some(datadir),
    );
    std::thread::sleep(Duration::from_millis(WAIT));

    stdout = output_contains("Broadcast this transaction for spend 0.10000000 BTC to bcrt1q6s79sgusgxzn25rve8nxfcu0fkddedg24hlavh", stdout);
    let spend_tx_1 = "02000000000103081060af91aa4cbbbb87f6d0945b56efed34454bd273b4ff570b430cdde2bd6d01000000000000000033eb2ef1f8f16823123c7c7ca44373d1061f4953c48371b904b37eb65201de64000000000000000000405a85a3ba4635ee2cc8384627ae3e98867726de69be8b2d46fac25da0995f1801000000000000000002d0455d050000000016001449169738dd4c330ab874fafa5f5fc4fd463b5d1a8096980000000000160014d43c582390418535506cc9e664e38f4d9adcb50a0247304402202c81a76a6bdb5a85fb7b99ca424d77f32f344e4672dbe31f921deab4c075bd06022074d4cf291a35cbfe2dc5d36080ec72faa9638f74ecc7dffd078189642a235e7b012103cd67ed4ef14f47076191acfb866837b1135ed03849b1b29bc4d73533e8a5dd7b02473044022004365eccfc2e765f527e1917ce4f40f27e4299625914462fb81ca6a82da4380702206c31ee48545cd1f0c2483c190585663b565c505bc1bbb8b4d6ead97e326e5c9b012103b0034d35dc1f0f8e90dab90161ad2195567c0c547fba15142c047f8fa7fa6a8002473044022078d1de27ab3bce676c8c186b429201ee74a05746a9d3d87f7b1d02795f394f9002207fffe78bcc6305b7222a4d7589a78218a806f41209106fa7a660f39286a114e501210209105f5fdb7949b65963d79aa5878201177bb1c47592bf94540a79b99529150d00000000";
    let _stdout = output_contains(spend_tx_1, stdout);

    // Register
    let (_stdin, stdout, _, datadir) = lfc(
        vec![
            "register".to_string(),
            "0".to_string(),
            spend_tx_1.to_string(),
        ],
        Some(datadir),
    );
    std::thread::sleep(Duration::from_millis(WAIT));

    let _stdout = output_contains("Spend registered", stdout);

    // List spendable
    let (_stdin, mut stdout, _, _datadir) = lfc(vec!["spendable".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(WAIT));

    // 1 coins is spendable
    stdout = output_contains("0.89998800", stdout);
    stdout = output_contains(
        "b77072e3942f4ca27a751f83076fb92d6e819134be86dd1b56f03ad763921abe:0: 0.89998800",
        stdout,
    );

    dump(1, &mut stdout);
}
