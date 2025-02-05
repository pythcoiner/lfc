mod cli;

use cli::commands::Command;

fn main() {
    let args = cli::parse();
    match &args.command {
        Command::Status => cli::commands::status(args),
        Command::Conf { .. } => cli::commands::conf(args),
        Command::Create => cli::commands::create(args),
        Command::Sign => cli::commands::sign(args),
        Command::Unlock => cli::commands::unlock(args),
        Command::Register {
            height,
            transaction,
        } => {
            let height = *height;
            let tx = transaction.clone();
            cli::commands::register(args, tx.clone(), height)
        }
        Command::Spend {
            amount: _,
            address: _,
        } => {}
        Command::Lock => todo!(),
        Command::Relock => todo!(),
        Command::Del => cli::commands::del_wallet(args),
    }
}
