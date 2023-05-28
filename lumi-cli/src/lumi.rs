use clap::{Parser, Subcommand};
use lumi::Ledger;

mod serve;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHOR: &str = env!("CARGO_PKG_AUTHORS");

fn files(ledger: Ledger) {
    for file in ledger.files() {
        println!("{}", file);
    }
}

fn balances(ledger: Ledger) {
    let mut result = vec![];
    for (account, account_map) in ledger.balance_sheet() {
        if ledger.accounts()[account].close().is_some() {
            continue;
        }
        for (currency, currency_map) in account_map {
            for (cost, number) in currency_map {
                if number.is_zero() {
                    continue;
                }
                if let Some(cost) = cost {
                    result.push(format!("{} {} {} {}", account, number, currency, cost));
                } else {
                    result.push(format!("{} {} {}", account, number, currency));
                }
            }
        }
    }
    result.sort();
    for entry in result {
        println!("{}", entry);
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "lumi",
    about = "A double-entry accounting tool.", 
    version = VERSION,
    author = AUTHOR,
)]
struct Cli {
    #[arg(short, required = true)]
    input: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Balances,
    Files,
    Serve {
        #[arg(short, long, default_value = "127.0.0.1:8001")]
        addr: String,
    },
}

fn main() -> std::io::Result<()> {
    let args = Cli::parse();
    let (ledger, errors) = Ledger::from_file(&args.input);
    for error in &errors {
        println!("{}\n", error);
    }
    match args.command {
        Commands::Balances => balances(ledger),
        Commands::Files => files(ledger),
        Commands::Serve { addr } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap();

            return runtime.block_on(serve::serve(addr, &args.input, ledger, errors));
        }
    }
    Ok(())
}
