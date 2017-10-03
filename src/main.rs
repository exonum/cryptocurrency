// Copyright 2017 The Exonum Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Import crates with necessary types into a new project.

extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate exonum;
extern crate router;
extern crate bodyparser;
extern crate iron;

// Import necessary types from crates.

use exonum::blockchain::{self, Blockchain, Service, GenesisConfig, ValidatorKeys, Transaction,
                         ApiContext};
use exonum::node::{Node, NodeConfig, NodeApiConfig, TransactionSend, ApiSender, NodeChannel};
use exonum::messages::{RawTransaction, FromRaw, Message};
use exonum::storage::{Snapshot, Fork, MemoryDB, ProofMapIndex};
use exonum::crypto::{PublicKey, Hash, HexValue};
use exonum::encoding;
use exonum::api::{Api, ApiError};
use iron::prelude::*;
use iron::Handler;
use router::Router;

use self::api::{BlockWithState, MapView};

mod api;

// // // // // // // // // // CONSTANTS // // // // // // // // // //

// Define service ID for the service trait.

const SERVICE_ID: u16 = 1;

// Define constants for transaction types within the service.

const TX_CREATE_WALLET_ID: u16 = 1;

const TX_TRANSFER_ID: u16 = 2;

// Define initial balance of a newly created wallet.

const INIT_BALANCE: u64 = 100;

// // // // // // // // // // PERSISTENT DATA // // // // // // // // // //

// Declare the data to be stored in the blockchain. In the present case,
// declare a type for storing information about the wallet and its balance.

/// Declare a [serializable][1]
/// [1]: https://github.com/exonum/exonum-doc/blob/master/src/architecture/serialization.md
/// struct and determine bounds of its fields with `encoding_struct!` macro.
encoding_struct! {
    struct Wallet {
        const SIZE = 48;

        field pub_key:            &PublicKey  [00 => 32]
        field name:               &str        [32 => 40]
        field balance:            u64         [40 => 48]
    }
}

/// Add methods to the `Wallet` type for changing balance.
impl Wallet {
    pub fn increase(self, amount: u64) -> Self {
        let balance = self.balance() + amount;
        Self::new(self.pub_key(), self.name(), balance)
    }

    pub fn decrease(self, amount: u64) -> Self {
        let balance = self.balance() - amount;
        Self::new(self.pub_key(), self.name(), balance)
    }
}

// // // // // // // // // // DATA LAYOUT // // // // // // // // // //

/// Create schema of the key-value storage implemented by `MemoryDB`. In the
/// present case a `Fork` of the database is used.
pub struct CurrencySchema<T> {
    view: T,
}

impl<T: AsRef<Snapshot>> CurrencySchema<T> {
    pub fn ro(snapshot: T) -> Self {
        CurrencySchema { view: snapshot }
    }

    pub fn wallets(&self) -> ProofMapIndex<&Snapshot, PublicKey, Wallet> {
        let prefix = blockchain::gen_prefix(SERVICE_ID, 0, &());
        let view: &Snapshot = self.view.as_ref();
        ProofMapIndex::new(prefix, view)
    }

    pub fn wallet(&self, key: &PublicKey) -> Option<Wallet> {
        return self.wallets().get(key);
    }

    pub fn state_hash(&self) -> Vec<Hash> {
        return vec![self.wallets().root_hash()];
    }
}

/// Declare layout of the data. Use an instance of [`ProofMapIndex`][2]
/// [2]: https://github.com/exonum/exonum-doc/blob/master/src/architecture/storage.md#proofmapindex
/// to keep wallets in storage. Index values are serialized `Wallet` structs.
///
/// Isolate the wallets map into a separate entity by adding a unique prefix,
/// i.e. the first argument to the `ProofMapIndex::new` call.
impl<'a> CurrencySchema<&'a mut Fork> {
    pub fn rw(fork: &'a mut Fork) -> Self {
        CurrencySchema { view: fork }
    }

    pub fn wallets_mut(&mut self) -> ProofMapIndex<&mut Fork, PublicKey, Wallet> {
        let prefix = blockchain::gen_prefix(SERVICE_ID, 0, &());
        ProofMapIndex::new(prefix, self.view)
    }
}

// // // // // // // // // // TRANSACTIONS // // // // // // // // // //

/// Create a new wallet.
message! {
    struct TxCreateWallet {
        const TYPE = SERVICE_ID;
        const ID = TX_CREATE_WALLET_ID;
        const SIZE = 40;

        field pub_key:     &PublicKey  [00 => 32]
        field name:        &str        [32 => 40]
    }
}

/// Transfer coins between the wallets.
message! {
    struct TxTransfer {
        const TYPE = SERVICE_ID;
        const ID = TX_TRANSFER_ID;
        const SIZE = 80;

        field from:        &PublicKey  [00 => 32]
        field to:          &PublicKey  [32 => 64]
        field amount:      u64         [64 => 72]
        field seed:        u64         [72 => 80]
    }
}

// // // // // // // // // // CONTRACTS // // // // // // // // // //

/// Execute a transaction.
impl Transaction for TxCreateWallet {
    /// Verify integrity of the transaction by checking the transaction
    /// signature.
    fn verify(&self) -> bool {
        self.verify_signature(self.pub_key())
    }

    /// Apply logic to the storage when executing the transaction.
    fn execute(&self, view: &mut Fork) {
        let mut schema = CurrencySchema::rw(view);
        if schema.wallet(self.pub_key()).is_none() {
            let wallet = Wallet::new(self.pub_key(), self.name(), INIT_BALANCE);
            println!("Create the wallet: {:?}", wallet);
            schema.wallets_mut().put(self.pub_key(), wallet)
        }
    }
}

impl Transaction for TxTransfer {
    /// Check if the sender is not the receiver. Check correctness of the
    /// sender's signature.
    fn verify(&self) -> bool {
        (*self.from() != *self.to()) && self.verify_signature(self.from())
    }

    /// Retrieve two wallets to apply the transfer. Check the sender's
    /// balance and apply changes to the balances of the wallets.
    fn execute(&self, view: &mut Fork) {
        let mut schema = CurrencySchema::rw(view);

        let sender = schema.wallet(self.from());
        let receiver = schema.wallet(self.to());

        if let (Some(sender), Some(receiver)) = (sender, receiver) {
            let amount = self.amount();

            if sender.balance() >= amount {
                let sender = sender.decrease(amount);
                let receiver = receiver.increase(amount);
                println!("Transfer between wallets: {:?} => {:?}", sender, receiver);
                let mut wallets = schema.wallets_mut();
                wallets.put(self.from(), sender);
                wallets.put(self.to(), receiver);
            }
        }
    }
}

// // // // // // // // // // REST API // // // // // // // // // //

/// Implement the node API.
#[derive(Clone)]
struct CryptocurrencyApi {
    channel: ApiSender<NodeChannel>,
    blockchain: Blockchain,
}

/// Shortcut to get data on wallets.
impl CryptocurrencyApi {
    fn get_wallet_view(&self, pub_key: PublicKey) -> MapView<PublicKey, Wallet> {
        let snapshot = self.blockchain.snapshot();
        let schema = CurrencySchema::ro(&snapshot);
        MapView::new(&schema.wallets(), pub_key)
    }
}

/// The structure returned by the REST API.
#[derive(Serialize)]
struct TxInfo {
    tx_hash: Hash,
}

impl CryptocurrencyApi {
    fn process_transaction<T>(&self, req: &mut Request) -> IronResult<Response>
    where
        T: Transaction + Clone,
        for<'a> T: serde::Deserialize<'a>,
    {
        match req.get::<bodyparser::Struct<T>>() {
            Ok(Some(transaction)) => {
                let transaction: Box<Transaction> = Box::new(transaction);
                let tx_hash = transaction.hash();
                self.channel.send(transaction).map_err(ApiError::Events)?;
                let json = TxInfo { tx_hash };
                self.ok_response(&serde_json::to_value(&json).unwrap())
            }
            Ok(None) => Err(ApiError::IncorrectRequest("Empty request body".into()))?,
            Err(e) => Err(ApiError::IncorrectRequest(Box::new(e)))?,
        }
    }
}

/// Implement the `Api` trait.
/// `Api` facilitates conversion between transactions/read requests and REST
/// endpoints; for example, it parses `POSTed` JSON into the binary transaction
/// representation used in Exonum internally.
impl Api for CryptocurrencyApi {
    fn wire(&self, router: &mut Router) {
        let self_ = self.clone();
        let tx_create = move |req: &mut Request| -> IronResult<Response> {
            self_.process_transaction::<TxCreateWallet>(req)
        };

        let self_ = self.clone();
        let tx_transfer = move |req: &mut Request| -> IronResult<Response> {
            self_.process_transaction::<TxTransfer>(req)
        };

        // Gets status of the wallet corresponding to the public key.
        let self_ = self.clone();
        let wallet_info = move |req: &mut Request| -> IronResult<Response> {
            let wallet_key = req.extensions
                .get::<Router>()
                .expect("router::Params not in request extensions")
                .find("pub_key")
                .ok_or(ApiError::IncorrectRequest("Missing public key".into()))?;
            let public_key = PublicKey::from_hex(wallet_key).map_err(ApiError::FromHex)?;

            let wallet_view = self_.get_wallet_view(public_key);
            let block =
                BlockWithState::new(self_.blockchain.snapshot(), SERVICE_ID, 0, wallet_view);

            self_.ok_response(&serde_json::to_value(block).unwrap())
        };

        // Bind the transaction handler to a specific route.
        router.post("/v1/wallets", tx_create, "tx_create");
        router.post("/v1/wallets/transfer", tx_transfer, "tx_transfer");
        router.get("/v1/wallet/:pub_key", wallet_info, "wallet_info");
    }
}

// // // // // // // // // // SERVICE DECLARATION // // // // // // // // // //

/// Define the service.
struct CurrencyService;

/// Implement a `Service` trait for the service.
impl Service for CurrencyService {
    fn service_name(&self) -> &'static str {
        "cryptocurrency"
    }

    fn service_id(&self) -> u16 {
        SERVICE_ID
    }

    fn state_hash(&self, view: &Snapshot) -> Vec<Hash> {
        let schema = CurrencySchema::ro(view);
        schema.state_hash()
    }

    /// Implement a method to deserialize transactions coming to the node.
    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, encoding::Error> {
        let trans: Box<Transaction> = match raw.message_type() {
            TX_TRANSFER_ID => Box::new(TxTransfer::from_raw(raw)?),
            TX_CREATE_WALLET_ID => Box::new(TxCreateWallet::from_raw(raw)?),
            _ => {
                return Err(encoding::Error::IncorrectMessageType {
                    message_type: raw.message_type(),
                });
            }
        };
        Ok(trans)
    }

    /// Create a REST `Handler` to process web requests to the node.
    fn public_api_handler(&self, ctx: &ApiContext) -> Option<Box<Handler>> {
        let mut router = Router::new();
        let api = CryptocurrencyApi {
            channel: ctx.node_channel().clone(),
            blockchain: ctx.blockchain().clone(),
        };
        api.wire(&mut router);
        Some(Box::new(router))
    }
}

// // // // // // // // // // ENTRY POINT // // // // // // // // // //

fn main() {
    exonum::helpers::init_logger().unwrap();

    println!("Creating in-memory database...");
    let db = MemoryDB::new();
    let services: Vec<Box<Service>> = vec![Box::new(CurrencyService)];
    let blockchain = Blockchain::new(Box::new(db), services);

    let (consensus_public_key, consensus_secret_key) = exonum::crypto::gen_keypair();
    let (service_public_key, service_secret_key) = exonum::crypto::gen_keypair();

    let validator_keys = ValidatorKeys {
        consensus_key: consensus_public_key,
        service_key: service_public_key,
    };
    let genesis = GenesisConfig::new(vec![validator_keys].into_iter());

    let api_address = "0.0.0.0:8000".parse().unwrap();
    let api_cfg = NodeApiConfig {
        public_api_address: Some(api_address),
        ..Default::default()
    };

    let peer_address = "0.0.0.0:2000".parse().unwrap();

    let node_cfg = NodeConfig {
        listen_address: peer_address,
        peers: vec![],
        service_public_key,
        service_secret_key,
        consensus_public_key,
        consensus_secret_key,
        genesis,
        external_address: None,
        network: Default::default(),
        whitelist: Default::default(),
        api: api_cfg,
        mempool: Default::default(),
        services_configs: Default::default(),
    };

    println!("Starting a single node...");
    let mut node = Node::new(blockchain, node_cfg);

    println!("Blockchain is ready for transactions!");
    node.run().unwrap();
}
