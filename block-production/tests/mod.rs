use std::str::FromStr;
use std::sync::Arc;
use std::{convert::TryInto, time::Instant};

use beserial::Deserialize;
use nimiq_block::{Block, BlockError, ForkProof};
use nimiq_block_production::BlockProducer;
use nimiq_blockchain::{AbstractBlockchain, Blockchain, PushError, PushResult};
use nimiq_database::{
    lmdb::{open as LmdbFlags, LmdbEnvironment},
    volatile::VolatileEnvironment,
};
use nimiq_genesis::NetworkId;
use nimiq_hash::{Blake2bHash, Hash};
use nimiq_keys::{Address, KeyPair as SchnorrKeyPair, PrivateKey as SchnorrPrivateKey};
use nimiq_keys::{KeyPair, PrivateKey};
use nimiq_primitives::coin::Coin;
use nimiq_primitives::policy;
use nimiq_test_utils::blockchain::{
    fill_micro_blocks, sign_macro_block, sign_view_change, signing_key, voting_key,
};
use nimiq_transaction::Transaction;
use nimiq_transaction_builder::TransactionBuilder;
use nimiq_utils::time::OffsetTime;
use nimiq_vrf::VrfSeed;
use parking_lot::RwLock;
use rand::prelude::StdRng;
use rand::{RngCore, SeedableRng};

const ADDRESS: &str = "NQ20TSB0DFSMUH9C15GQGAGJTTE4D3MA859E";

pub const ACCOUNT_SECRET_KEY: &str =
    "6c9320ac201caf1f8eaa5b05f5d67a9e77826f3f6be266a0ecccc20416dc6587";

const STAKER_ADDRESS: &str = "NQ20TSB0DFSMUH9C15GQGAGJTTE4D3MA859E";
const UNIT_KEY: &str = "6c9320ac201caf1f8eaa5b05f5d67a9e77826f3f6be266a0ecccc20416dc6587";
const VOLATILE_ENV: bool = false;

#[test]
fn it_can_produce_micro_blocks() {
    let time = Arc::new(OffsetTime::new());
    let env = VolatileEnvironment::new(10).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(env, NetworkId::UnitAlbatross, time).unwrap(),
    ));
    let producer = BlockProducer::new(signing_key(), voting_key());

    let bc = blockchain.upgradable_read();

    // Store seed before pushing a block as it is needed for the fork proof.
    let prev_vrf_seed = bc.head().seed().clone();

    // #1.0: Empty standard micro block
    let block = producer.next_micro_block(&bc, bc.time.now(), 0, None, vec![], vec![], vec![0x41]);

    assert_eq!(
        Blockchain::push(bc, Block::Micro(block.clone())),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 1);

    // Create fork at #1.0
    let fork_proof = {
        let header1 = block.header.clone();
        let justification1 = block.justification.unwrap().signature;
        let mut header2 = header1.clone();
        header2.timestamp += 1;
        let hash2 = header2.hash::<Blake2bHash>();
        let justification2 = signing_key().sign(hash2.as_slice());
        ForkProof {
            header1,
            header2,
            justification1,
            justification2,
            prev_vrf_seed,
        }
    };

    let bc = blockchain.upgradable_read();
    // #2.0: Empty micro block with fork proof
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 1000,
        0,
        None,
        vec![fork_proof],
        vec![],
        vec![0x41],
    );
    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 2);
    assert_eq!(blockchain.read().view_number(), 0);

    // #2.1: Empty view-changed micro block (wrong prev_hash)
    let view_change = sign_view_change(VrfSeed::default(), 3, 1);
    let bc = blockchain.upgradable_read();
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 2000,
        1,
        Some(view_change),
        vec![],
        vec![],
        vec![0x41],
    );

    // the block justification is ok, the view_change justification is not.
    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Err(PushError::InvalidBlock(BlockError::InvalidViewChangeProof))
    );

    // #2.2: Empty view-changed micro block
    let view_change = sign_view_change(blockchain.read().head().seed().clone(), 3, 1);
    let bc = blockchain.upgradable_read();
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 2000,
        1,
        Some(view_change),
        vec![],
        vec![],
        vec![0x41],
    );
    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );
    assert_eq!(blockchain.read().block_number(), 3);
    assert_eq!(blockchain.read().next_view_number(), 1);
}

#[test]
fn it_can_produce_macro_blocks() {
    let time = Arc::new(OffsetTime::new());
    let env = VolatileEnvironment::new(10).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(env, NetworkId::UnitAlbatross, time).unwrap(),
    ));
    let producer = BlockProducer::new(signing_key(), voting_key());

    fill_micro_blocks(&producer, &blockchain);

    let bc = blockchain.upgradable_read();
    let macro_block = {
        producer.next_macro_block_proposal(
            &bc,
            bc.time.now() + bc.block_number() as u64 * 1000,
            0u32,
            vec![],
        )
    };

    let block = sign_macro_block(&voting_key(), macro_block.header, macro_block.body);
    assert_eq!(
        Blockchain::push(bc, Block::Macro(block)),
        Ok(PushResult::Extended)
    );
}

#[test]
fn it_can_produce_election_blocks() {
    let time = Arc::new(OffsetTime::new());
    let env = VolatileEnvironment::new(10).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(env, NetworkId::UnitAlbatross, time).unwrap(),
    ));
    let producer = BlockProducer::new(signing_key(), voting_key());

    // push micro and macro blocks until the 3rd epoch is reached
    while policy::epoch_at(blockchain.read().block_number()) < 2 {
        fill_micro_blocks(&producer, &blockchain);

        let bc = blockchain.upgradable_read();
        let macro_block = {
            producer.next_macro_block_proposal(
                &bc,
                bc.time.now() + bc.block_number() as u64 * 1000,
                0u32,
                vec![0x42],
            )
        };

        let block = sign_macro_block(&voting_key(), macro_block.header, macro_block.body);

        assert_eq!(
            Blockchain::push(bc, Block::Macro(block)),
            Ok(PushResult::Extended)
        );
    }
}

fn generate_transactions(
    key_pair: &KeyPair,
    start_height: u32,
    network_id: NetworkId,
    count: usize,
) -> Vec<Transaction> {
    let mut txs = Vec::new();

    let mut rng = StdRng::seed_from_u64(0);
    for _ in 0..count {
        let mut bytes = [0u8; 20];
        rng.fill_bytes(&mut bytes);
        let recipient = Address::from(bytes);

        let tx = TransactionBuilder::new_basic(
            key_pair,
            recipient,
            Coin::from_u64_unchecked(1),
            Coin::from_u64_unchecked(2),
            start_height,
            network_id,
        );
        txs.push(tx);
    }

    txs
}

pub fn fill_micro_blocks_with_txns(producer: &BlockProducer, blockchain: &Arc<RwLock<Blockchain>>) {
    let init_height = blockchain.read().block_number();
    let key_pair = KeyPair::from(PrivateKey::from_str(UNIT_KEY).unwrap());
    assert!(policy::is_macro_block_at(init_height));

    let macro_block_number = init_height + policy::BATCH_LENGTH;

    for i in (init_height + 1)..macro_block_number {
        println!(" Current Height: {}", i);
        let blockchain = blockchain.upgradable_read();

        //Generate the transactions
        let txns = generate_transactions(&key_pair, i, NetworkId::UnitAlbatross, 700);
        let start = Instant::now();
        let last_micro_block = producer.next_micro_block(
            &blockchain,
            blockchain.time.now() + i as u64 * 100,
            0,
            None,
            vec![],
            txns,
            vec![0x42],
        );
        let duration = start.elapsed();
        println!(
            "   Time elapsed pushing micro: {} ms, ",
            duration.as_millis(),
        );

        assert_eq!(
            Blockchain::push(blockchain, Block::Micro(last_micro_block)),
            Ok(PushResult::Extended)
        );
    }

    assert_eq!(blockchain.read().block_number(), macro_block_number - 1);
}

#[test]
fn it_can_produce_a_chain_with_txns() {
    let time = Arc::new(OffsetTime::new());
    let env = if VOLATILE_ENV {
        VolatileEnvironment::new(10).unwrap()
    } else {
        //let tmp_dir =
        //    TempDir::new("accounts_performance_test").expect("Could not create temporal directory");
        //let tmp_dir = tmp_dir.path().to_str().unwrap();
        let tmp_dir = "/Users/claudioviquez/Workspace/db";
        println!("Creating a non volatile environment in {}", tmp_dir);
        LmdbEnvironment::new(
            tmp_dir,
            1024 * 1024 * 1024 * 1024,
            21,
            LmdbFlags::NOMETASYNC | LmdbFlags::NOSYNC,
        )
        .unwrap()
    };
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(env, NetworkId::UnitAlbatross, time).unwrap(),
    ));
    let producer = BlockProducer::new(signing_key(), voting_key());

    loop {
        fill_micro_blocks_with_txns(&producer, &blockchain);

        let blockchain = blockchain.upgradable_read();
        let next_block_height = (blockchain.block_number() + 1) as u64;

        let macro_block_proposal = producer.next_macro_block_proposal(
            &blockchain,
            blockchain.time.now() + next_block_height as u64 * 100,
            0u32,
            vec![],
        );

        let block = sign_macro_block(
            &producer.voting_key,
            macro_block_proposal.header,
            macro_block_proposal.body,
        );

        assert_eq!(
            Blockchain::push(blockchain, Block::Macro(block)),
            Ok(PushResult::Extended)
        );
    }
}

#[test]
fn it_can_revert_unpark_transactions() {
    let time = Arc::new(OffsetTime::new());
    let env = VolatileEnvironment::new(10).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(env, NetworkId::UnitAlbatross, time).unwrap(),
    ));
    let producer = BlockProducer::new(signing_key(), voting_key());

    // #1.0: Empty view-changed micro block
    let view_change = sign_view_change(blockchain.read().head().seed().clone(), 1, 1);
    let bc = blockchain.upgradable_read();

    let block = producer.next_micro_block(
        &bc,
        bc.time.now(),
        1,
        Some(view_change),
        vec![],
        vec![],
        vec![0x41],
    );

    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 1);
    assert_eq!(blockchain.read().next_view_number(), 1);

    let bc = blockchain.upgradable_read();

    // One empty block
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 2000,
        1,
        None,
        vec![],
        vec![],
        vec![0x41],
    );

    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 2);
    assert_eq!(blockchain.read().next_view_number(), 1);

    // One block with stacking transactions

    let mut transactions = vec![];
    let key_pair = signing_key();
    let address = Address::from_any_str(ADDRESS).unwrap();

    let tx = TransactionBuilder::new_unpark_validator(
        &key_pair,
        address,
        &key_pair,
        Coin::ZERO,
        1,
        NetworkId::UnitAlbatross,
    );

    transactions.push(tx);

    let bc = blockchain.upgradable_read();

    // Block with stacking transactions
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 2000,
        1,
        None,
        vec![],
        transactions,
        vec![0x41],
    );

    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 3);
    assert_eq!(blockchain.read().next_view_number(), 1);

    let bc = blockchain.upgradable_read();

    let mut txn = bc.write_transaction();

    let result = bc.revert_blocks(3, &mut txn);

    assert_eq!(result, Ok(()));
}

#[test]
fn it_can_revert_create_stacker_transaction() {
    let time = Arc::new(OffsetTime::new());
    let env = VolatileEnvironment::new(10).unwrap();
    let blockchain = Arc::new(RwLock::new(
        Blockchain::new(env, NetworkId::UnitAlbatross, time).unwrap(),
    ));
    let producer = BlockProducer::new(signing_key(), voting_key());

    // #1.0: Empty view-changed micro block
    let view_change = sign_view_change(blockchain.read().head().seed().clone(), 1, 1);
    let bc = blockchain.upgradable_read();

    let block = producer.next_micro_block(
        &bc,
        bc.time.now(),
        1,
        Some(view_change),
        vec![],
        vec![],
        vec![0x41],
    );
    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );
    assert_eq!(blockchain.read().block_number(), 1);
    assert_eq!(blockchain.read().next_view_number(), 1);

    let bc = blockchain.upgradable_read();

    // One empty block
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 2000,
        1,
        None,
        vec![],
        vec![],
        vec![0x41],
    );

    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 2);
    assert_eq!(blockchain.read().next_view_number(), 1);

    // One block with stacking transactions

    let mut transactions = vec![];
    let key_pair = ed25519_key_pair(ACCOUNT_SECRET_KEY);
    let address = Address::from_any_str(STAKER_ADDRESS).unwrap();

    let tx = TransactionBuilder::new_create_staker(
        &key_pair,
        &key_pair,
        Some(address),
        100_000_000.try_into().unwrap(),
        100.try_into().unwrap(),
        1,
        NetworkId::UnitAlbatross,
    );

    transactions.push(tx);

    let bc = blockchain.upgradable_read();

    // Block with stacking transactions
    let block = producer.next_micro_block(
        &bc,
        bc.time.now() + 2000,
        1,
        None,
        vec![],
        transactions,
        vec![0x41],
    );

    assert_eq!(
        Blockchain::push(bc, Block::Micro(block)),
        Ok(PushResult::Extended)
    );

    assert_eq!(blockchain.read().block_number(), 3);
    assert_eq!(blockchain.read().next_view_number(), 1);

    let bc = blockchain.upgradable_read();

    let mut txn = bc.write_transaction();
    let result = bc.revert_blocks(3, &mut txn);

    assert_eq!(result, Ok(()));
}

fn ed25519_key_pair(secret_key: &str) -> SchnorrKeyPair {
    let priv_key: SchnorrPrivateKey =
        Deserialize::deserialize(&mut &hex::decode(secret_key).unwrap()[..]).unwrap();
    priv_key.into()
}
