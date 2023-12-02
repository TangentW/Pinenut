use base64::{prelude::BASE64_STANDARD, Engine};
use clap::{Args, Parser, Subcommand};
use pinenut_log::DefaultFormatter;

#[derive(Parser)]
#[command(about = "The Pinenut command line tool.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generates the ECHD key pair.
    GenKeys(GenKeys),
    /// Parses the compressed and encrypted binary log file into readable text file.
    Parse(Parse),
}

#[derive(Args)]
struct GenKeys;

impl GenKeys {
    fn exec(self) {
        let (secret_key, public_key) = pinenut_log::encrypt::gen_echd_key_pair();
        let secret_key = BASE64_STANDARD.encode(secret_key);
        let public_key = BASE64_STANDARD.encode(public_key);

        println!("ECDH Keys:");
        println!("-----------");
        println!("Secret Key: {}", secret_key);
        println!("Public Key: {}", public_key);
    }
}

#[derive(Args)]
struct Parse {
    /// Path to log File.
    path: String,
    /// Path to destnation File.
    ///
    /// If it is not specified, the default `.log` file is generated in the same
    /// directory as `path`.
    #[arg(short, long)]
    output: Option<String>,
    /// The secret key.
    #[arg(short, long)]
    secret_key: Option<String>,
}

impl Parse {
    fn exec(self) {
        println!("Parsing ...");
        let output = self.output.unwrap_or_else(|| self.path.clone() + ".log");
        let secret_key = self
            .secret_key
            .and_then(|k| BASE64_STANDARD.decode(k).ok())
            .and_then(|k| k.try_into().ok());
        let res = pinenut_log::parse_to_file(&self.path, output, secret_key, DefaultFormatter);
        if let Err(err) = res {
            println!("Error: {err}");
        }
    }
}

impl Command {
    #[inline]
    fn exec(self) {
        match self {
            Self::GenKeys(gen_keys) => gen_keys.exec(),
            Self::Parse(parse) => parse.exec(),
        }
    }
}

fn main() {
    Cli::parse().command.exec();
}
