use exonum::blockchain::{self, Block};
use exonum::crypto::Hash;
use exonum::helpers::Height;
use exonum::messages::Precommit;
use exonum::storage::{Snapshot, ProofMapIndex, MapProof, StorageValue};
use exonum::storage::proof_map_index::ProofMapKey;

use serde::{Serialize, Serializer};
use serde::ser::SerializeStruct;

#[derive(Serialize)]
pub struct StateTableKey {
    pub service_id: u16,
    pub table_id: usize,
}

#[derive(Serialize)]
pub struct KeyValuePair<K: Serialize, V: Serialize> {
    key: K,
    value: V,
}

impl<K: Serialize, V: Serialize> KeyValuePair<K, V> {
    fn new(key: K, value: V) -> Self {
        KeyValuePair { key, value }
    }
}

#[derive(Serialize)]
struct StateView<T: Serialize> {
    proof: MapProof<Hash>,
    entries: Vec<KeyValuePair<StateTableKey, T>>,
}

#[derive(Serialize)]
pub struct MapView<K: Serialize, V: Serialize> {
    proof: MapProof<V>,
    entries: Vec<KeyValuePair<K, V>>,
}

impl<K, V> MapView<K, V>
where
    K: ProofMapKey + Serialize,
    V: StorageValue + Serialize,
{
    pub fn new<T: AsRef<Snapshot>>(table: &ProofMapIndex<T, K, V>, key: K) -> Self {
        let val = table.get(&key);

        MapView {
            proof: table.get_proof(&key),
            entries: if let Some(val) = val {
                vec![KeyValuePair::new(key, val)]
            } else {
                vec![]
            },
        }
    }
}

pub struct BlockWithState<T: Serialize> {
    block: Block,
    precommits: Vec<Precommit>,
    state: StateView<T>,
}

impl<T: Serialize> BlockWithState<T> {
    pub fn new<S: AsRef<Snapshot>>(
        snapshot: S,
        service_id: u16,
        table_id: usize,
        table_view: T,
    ) -> Self {
        let table_key = StateTableKey {
            service_id,
            table_id,
        };
        let schema = blockchain::Schema::new(&snapshot);
        let max_height = schema.block_hashes_by_height().len() - 1;
        let block_proof = schema.block_and_precommits(Height(max_height)).unwrap();

        let proof_to_table = schema.get_proof_to_service_table(service_id, table_id);

        BlockWithState {
            block: block_proof.block,
            precommits: block_proof.precommits,
            state: StateView {
                proof: proof_to_table,
                entries: vec![KeyValuePair::new(table_key, table_view)],
            },
        }
    }
}

impl<T: Serialize> Serialize for BlockWithState<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut obj = serializer.serialize_struct("BlockWithState", 8)?;
        obj.serialize_field("height", &self.block.height())?;
        obj.serialize_field("prev_hash", &self.block.prev_hash())?;
        obj.serialize_field(
            "schema_version",
            &self.block.schema_version(),
        )?;
        obj.serialize_field(
            "proposer_id",
            &self.block.proposer_id(),
        )?;
        obj.serialize_field("tx_count", &self.block.tx_count())?;
        obj.serialize_field("tx_hash", &self.block.tx_hash())?;
        obj.serialize_field("precommits", &self.precommits)?;
        obj.serialize_field("state", &self.state)?;
        obj.end()
    }
}
