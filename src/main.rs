use chrono::{NaiveDateTime, Utc};
use env_file_reader::read_file;
use ethers::{
    abi::{Abi, Token, Tokenizable},
    contract::Contract,
    prelude::*,
    signers::{coins_bip39::English, MnemonicBuilder},
};
use gql_client::Client;
use gumdrop::Options;
use serde::Deserialize;
use std::convert::TryFrom;

#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
struct Gotchi {
    id: String,
    lastInteracted: String,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
struct User {
    gotchisOwned: Vec<Gotchi>,
}

#[derive(Deserialize, Debug)]
struct Data {
    user: User,
}

#[derive(Debug, Options, Clone)]
struct Opts {
    help: bool,

    #[options(
        help = "the Ethereum node endpoint (HTTP or WS)",
        default = "http://localhost:8545"
    )]
    url: String,
}

const DIAMOND_ADDRESS: &str = "0x86935F11C86623deC8a25696E1C19a8659CbF95d";
const SUBGRAPH_URL: &str =
    "https://api.thegraph.com/subgraphs/name/aavegotchi/aavegotchi-core-matic";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse_args_default_or_exit();
    let vars = read_file(".env")?;

    let seed = vars["SECRET"].as_str();
    let wallet = MnemonicBuilder::<English>::default().phrase(seed).build()?;

    let address = wallet.address();
    let query = format!(
        "
    query Query {{
        user(id: \"{:?}\") {{
            gotchisOwned(first: 1000, orderBy: lastInteracted, where: {{baseRarityScore_gt: 0}}) {{
                id
                lastInteracted
            }}
        }}
    }}
    ",
        address
    );

    let client = Client::new(SUBGRAPH_URL);
    let response = client.query::<Data>(&query).await.unwrap().unwrap();

    let to_pet: Vec<Token> = response
        .user
        .gotchisOwned
        .iter()
        .filter(|gotchi| {
            let last_pet: i64 = gotchi.lastInteracted.trim().parse().unwrap();
            let date_time_next_pet = NaiveDateTime::from_timestamp(last_pet + (60 * 60 * 12), 0);
            Utc::now().naive_utc() > date_time_next_pet
        })
        .map(|x| U256::from(x.id.parse::<i32>().unwrap()).into_token())
        .collect();

    if to_pet.len() > 1 {
        let provider = Provider::<Http>::try_from(opts.url.clone())?;
        let signing_client = SignerMiddleware::new_with_provider_chain(provider, wallet).await?;

        let aavegotchi_diamond: Address = DIAMOND_ADDRESS.parse::<Address>()?;
        let abi_data = std::fs::read_to_string("./abis/diamond.json")?;
        let abi: Abi = serde_json::from_str(&abi_data)?;
        let contract = Contract::new(aavegotchi_diamond, abi, signing_client);

        let args: Token = Token::Array(to_pet);
        let call = contract.method::<_, ()>("interact", args)?;

        let pending_tx = call.send().await?;
        println!("pending_tx {:#?}", pending_tx);
        let receipt = pending_tx.confirmations(1).await?;
        println!("receipt {:#?}", receipt.unwrap().transaction_hash);
    }

    Ok(())
}
