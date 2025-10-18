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
    std::thread::sleep(Duration::from_millis(200));
    let _stdout = output_contains("Are you sure to delete wallet", stdout);
    send_stdin("n", &mut stdin);
    std::thread::sleep(Duration::from_millis(200));
    assert!(dir.exists());

    // NOTE: _tmp must note be drop else we have a false positive of deletion
    let (mut stdin, stdout, _, _tmp) = lfc(vec!["del".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(200));
    let _stdout = output_contains("Are you sure to delete wallet", stdout);
    send_stdin("y", &mut stdin);

    std::thread::sleep(Duration::from_millis(200));
    assert!(!dir.exists());
}

#[test]
fn test_create_sign_unlock() {
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
    std::thread::sleep(Duration::from_millis(200));

    stdout = output_contains(
        "Address to fund the contract: bcrt1q70rqqql720wv2gr0q8u3rleqqu7h05e7z88mvg",
        stdout,
    );
    stdout = output_contains("Enter raw tx that fund the contract", stdout);

    let funding_tx = "020000000001018dd2f188c07d64feb5590f05becc1393cd7874193896becb96ca6fc03a554e9c0100000000fdffffff0200e1f50500000000160014f3c60003fe53dcc5206f01f911ff20073d77d33e5ee8a43500000000220020255487893037882f10106d26fd0769bbdb967b8a82cb90faa9dcdaa3ae0cb1890247304402202b6c623d2bbcd96ba8c0f4ee1b3e1be648083e266a44d22e9decc6802e03e32b022054a6ac26661e230066d8b4a406156821e14d0a69ffe699b0113ead4d8b7c6c49014421025f222dcd8035d3d5a08db9b93273b0194826a686eb7ba402e75b92c7cd9bebfbac736476a914d041e63340f920609e14e91a5ed705b8c796c20c88ad0374cd00b268e8020000";
    send_stdin(funding_tx, &mut stdin);

    let mut buf = String::new();
    for _ in 0..3 {
        let _ = stdout.read_line(&mut buf);
    }

    let _stdout = output_contains("Successfully generated 3 rounds!", stdout);

    // Sign
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["sign".to_string()], Some(datadir.clone()));
    std::thread::sleep(Duration::from_millis(200));

    let mut buf = String::new();
    for _ in 0..3 {
        let _ = stdout.read_line(&mut buf);
    }

    let _stdout = output_contains("Successfully signed 3 rounds!", stdout);

    // Unlock round 0
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(200));

    stdout = output_contains(
        "Broadcast this transaction to unlock the current round",
        stdout,
    );

    let unlock_tx = "02000000000101acdd26f0527f7cb38893c8b43fcfa763608eb63a75f9c963ad91f24474627f2e00000000000000000002aad1df0300000000160014c0585c8ba63c22a2ed2592a5d35208cbde64643fc00e1602000000001600143e8afa59f9e340af7a5018f399e0e00d05c542620247304402206e7e723afe1c268bc528d6fdd79138b79f541e6be28d737f55cd7edb11fb848602202ec5626293e29d9c64b32024dd172711710bf973e037fc8bd888e5d74eebdc7b012103407c3190500a371290d5654ea51d986afcf405fc10bd0131b8dfb800e61f3ee600000000";
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
    std::thread::sleep(Duration::from_millis(200));

    let _stdout = output_contains("Unlock registered", stdout);

    // Unlock round 1
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(200));

    stdout = output_contains(
        "Broadcast this transaction to unlock the next round after block height ",
        stdout,
    );

    let unlock_tx_2 = "0200000000010171e4500f6e35854874835df6ef6ffcbe2ece3bee189b1ea33bd65d0db47918c70000000000640000000254c2c901000000001600149a115404fedcd2525639c7ce8e6bcab96d062a74c00e1602000000001600145452c0a91064bf144f74fba548bd31c98909b0df0247304402205221e58ef139c7c78cb469d407ec38c7a46d064434021655691d26f16f30d6ac022006270fa1eb7a2ed338e703dc6639972175f06de4a163dd33a947a92f79c9860d012102daf01b75eee0d904ab3d05c903976eaaa5373191e552ae73312916c719f1918900000000";
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
    std::thread::sleep(Duration::from_millis(200));

    let _stdout = output_contains("Unlock registered", stdout);

    // Unlock round 2
    let (_stdin, mut stdout, _, datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(200));

    stdout = output_contains(
        "Broadcast this transaction to unlock the next round after block height ",
        stdout,
    );

    let unlock_tx_3 = "020000000001016cf08ff2e15f3b9eaa3515fc2c681b489ed7af1cb69b04502b6b7544381cc5d20000000000640000000166bfc90100000000160014195dc81d4622339e0676721705ad05dc719fa98302473044022003e147507a19f7369a719b9eb5b6c1253089b4457383f209da89b205c9074ae80220247020b2e6c8d5c8667e5dbd0d68f66d14ce7e629e16608d46142831f1950098012102b7be4e1593ea444428da822b31da0581477ced632a38139d02e46dc690f8dde200000000";
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
    std::thread::sleep(Duration::from_millis(200));

    let _stdout = output_contains("Unlock registered", stdout);

    // Nothing to unlock
    let (_stdin, mut stdout, _, _datadir) = lfc(vec!["unlock".to_string()], Some(datadir));
    std::thread::sleep(Duration::from_millis(200));

    stdout = output_contains("No more round to unlock", stdout);

    dump(10, &mut stdout);
}
