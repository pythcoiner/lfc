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
) -> (
    BufWriter<PipeWriter>,
    BufReader<PipeReader>,
    JoinHandle<Output>,
    TempDir,
) {
    let tmp_dir = temp_dir::TempDir::new().unwrap();
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

fn print_line(stdout: &mut BufReader<PipeReader>) {
    let mut str = String::new();
    if let Ok(l) = stdout.read_line(&mut str) {
        if l > 0 {
            println!("{str}");
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
    let prompt = prompt.unwrap();
    assert!(prompt.contains(text));
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

#[test]
fn test_cli_conf() {
    let (mut stdin, mut stdout, _handle, tmp_dir) = lfc(vec!["conf".to_string()]);

    stdout = output_contains("Select a derivation index for the wallet", stdout);
    send_stdin("0", &mut stdin);

    stdout = output_contains("Enter the mnemonic for your covenant wallet", stdout);
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
    assert!(!state.cov_mnemonic.is_empty());
    assert!(!state.spend_mnemonic.is_empty());
    assert_eq!(state.amount, 1_000_000);
    assert_eq!(state.delay, 100);

    // let stop = time::SystemTime::now()
    //     .checked_add(Duration::from_secs(5))
    //     .unwrap();
    // while time::SystemTime::now() < stop {
    //     print_line(&mut stdout);
    // }
}
