use beserial::Serialize;
use nimiq_account::{Accounts, Inherent, InherentType};
use nimiq_account::{Receipt, Receipts};
use nimiq_bls::KeyPair as BLSKeyPair;
use nimiq_build_tools::genesis::GenesisBuilder;
use nimiq_database::WriteTransaction;
use nimiq_database::{
    lmdb::{open as LmdbFlags, LmdbEnvironment},
    volatile::VolatileEnvironment,
};
use nimiq_keys::{Address, KeyPair, PublicKey, SecureGenerate};
use nimiq_primitives::coin::Coin;
use nimiq_primitives::networks::NetworkId;
use nimiq_transaction::Transaction;
use nimiq_trie::key_nibbles::KeyNibbles;
use rand::prelude::StdRng;
use rand::{RngCore, SeedableRng};
use std::convert::TryFrom;
use std::time::Instant;
use tempdir::TempDir;
use unroll::unroll_for_loops;

const VOLATILE_ENV: bool = false;

#[derive(Clone)]
struct MempoolAccount {
    keypair: KeyPair,
    address: Address,
}

#[derive(Clone)]
struct MempoolTransaction {
    fee: u64,
    value: u64,
    sender: MempoolAccount,
    recipient: MempoolAccount,
}

fn generate_accounts(
    balances: Vec<u64>,
    genesis_builder: &mut GenesisBuilder,
    add_to_genesis: bool,
) -> Vec<MempoolAccount> {
    let mut mempool_accounts = vec![];

    for i in 0..balances.len() as usize {
        // Generate the txns_sender and txns_rec vectors to later generate transactions
        let keypair = KeyPair::generate_default_csprng();
        let address = Address::from(&keypair.public);
        let mempool_account = MempoolAccount {
            keypair,
            address: address.clone(),
        };
        mempool_accounts.push(mempool_account);

        if add_to_genesis {
            // Add accounts to the genesis builder
            genesis_builder.with_basic_account(address, Coin::from_u64_unchecked(balances[i]));
        }
    }
    mempool_accounts
}

fn generate_transactions(
    mempool_transactions: Vec<MempoolTransaction>,
    mut random_seed: StdRng,
) -> (Vec<Transaction>, usize) {
    let mut txns_len = 0;
    let mut txns: Vec<Transaction> = vec![];

    println!("Generating transactions and accounts");

    for mempool_transaction in mempool_transactions {
        let mut bytes = [0u8; 20];
        random_seed.fill_bytes(&mut bytes);
        let recipient = Address::from(bytes);
        // println!(" Recipient Address: {} ", recipient);
        // Generate transactions
        let txn = Transaction::new_basic(
            mempool_transaction.sender.address.clone(),
            //mempool_transaction.recipient.address.clone(),
            recipient,
            Coin::from_u64_unchecked(mempool_transaction.value),
            Coin::from_u64_unchecked(mempool_transaction.fee),
            1,
            NetworkId::UnitAlbatross,
        );

        //let signature_proof = SignatureProof::from(
        //    mempool_transaction.sender.keypair.public,
        //    mempool_transaction
        //        .sender
        //        .keypair
        //        .sign(&txn.serialize_content()),
        //);
        //
        //txn.proof = signature_proof.serialize_to_vec();
        txns.push(txn.clone());
        txns_len += txn.serialized_size();
    }
    (txns, txns_len)
}

#[test]
fn it_can_commit_and_revert_a_block_body() {
    let env = VolatileEnvironment::new(10).unwrap();

    let accounts = Accounts::new(env.clone());

    let address_validator = Address::from([1u8; Address::SIZE]);

    let address_recipient = Address::from([2u8; Address::SIZE]);

    let reward = Inherent {
        ty: InherentType::Reward,
        target: address_validator.clone(),
        value: Coin::from_u64_unchecked(10000),
        data: vec![],
    };

    let mut receipts = vec![Receipt::Inherent {
        index: 0,
        pre_transactions: false,
        data: None,
    }];

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_validator), None),
        None
    );

    let mut txn = WriteTransaction::new(&env);

    assert_eq!(
        accounts.commit(&mut txn, &[], &[reward.clone()], 1, 1),
        Ok(Receipts::from(receipts.clone()))
    );

    txn.commit();

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_validator), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000)
    );

    let hash1 = accounts.get_root(None);

    let tx = Transaction::new_basic(
        address_validator.clone(),
        address_recipient.clone(),
        Coin::from_u64_unchecked(10),
        Coin::ZERO,
        1,
        NetworkId::Main,
    );

    let transactions = vec![tx];

    receipts.insert(
        0,
        Receipt::Transaction {
            index: 0,
            sender: false,
            data: None,
        },
    );

    receipts.insert(
        0,
        Receipt::Transaction {
            index: 0,
            sender: true,
            data: None,
        },
    );

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    let mut txn = WriteTransaction::new(&env);

    assert_eq!(
        accounts.commit(&mut txn, &transactions, &[reward.clone()], 2, 2),
        Ok(Receipts::from(receipts.clone()))
    );

    txn.commit();

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_recipient), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10)
    );

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_validator), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000 + 10000 - 10)
    );

    assert_ne!(hash1, accounts.get_root(None));

    let mut txn = WriteTransaction::new(&env);

    assert_eq!(
        accounts.revert(
            &mut txn,
            &transactions,
            &[reward],
            2,
            2,
            &Receipts::from(receipts)
        ),
        Ok(())
    );

    txn.commit();

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_validator), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000)
    );

    assert_eq!(hash1, accounts.get_root(None));
}

#[test]
fn it_correctly_rewards_validators() {
    let env = VolatileEnvironment::new(10).unwrap();

    let accounts = Accounts::new(env.clone());

    let address_validator_1 = Address::from([1u8; Address::SIZE]);

    let address_validator_2 = Address::from([2u8; Address::SIZE]);

    let address_recipient_1 = Address::from([3u8; Address::SIZE]);

    let address_recipient_2 = Address::from([4u8; Address::SIZE]);

    // Validator 1 mines first block.
    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_validator_1), None),
        None
    );

    let reward = Inherent {
        ty: InherentType::Reward,
        target: address_validator_1.clone(),
        value: Coin::from_u64_unchecked(10000),
        data: vec![],
    };

    let mut txn = WriteTransaction::new(&env);

    assert!(accounts.commit(&mut txn, &[], &[reward], 1, 1).is_ok());

    txn.commit();

    // Create transactions to Recipient 1 and Recipient 2.
    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_validator_1), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000)
    );

    let value1 = Coin::from_u64_unchecked(5);

    let fee1 = Coin::from_u64_unchecked(3);

    let value2 = Coin::from_u64_unchecked(7);

    let fee2 = Coin::from_u64_unchecked(11);

    let tx1 = Transaction::new_basic(
        address_validator_1.clone(),
        address_recipient_1.clone(),
        value1,
        fee1,
        2,
        NetworkId::Main,
    );

    let tx2 = Transaction::new_basic(
        address_validator_1.clone(),
        address_recipient_2.clone(),
        value2,
        fee2,
        2,
        NetworkId::Main,
    );

    // Validator 2 mines second block.
    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_validator_2), None),
        None
    );

    let reward = Inherent {
        ty: InherentType::Reward,
        target: address_validator_2.clone(),
        value: Coin::from_u64_unchecked(10000) + fee1 + fee2,
        data: vec![],
    };

    let mut txn = WriteTransaction::new(&env);

    assert!(accounts
        .commit(&mut txn, &vec![tx1, tx2], &[reward], 2, 2)
        .is_ok());

    txn.commit();

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_validator_1), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000) - value1 - fee1 - value2 - fee2
    );

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_validator_2), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000) + fee1 + fee2
    );

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_recipient_1), None)
            .unwrap()
            .balance(),
        value1
    );

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_recipient_2), None)
            .unwrap()
            .balance(),
        value2
    );
}

#[test]
fn it_checks_for_sufficient_funds() {
    let env = VolatileEnvironment::new(10).unwrap();

    let accounts = Accounts::new(env.clone());

    let address_sender = Address::from([1u8; Address::SIZE]);

    let address_recipient = Address::from([2u8; Address::SIZE]);

    let mut tx = Transaction::new_basic(
        address_sender.clone(),
        address_recipient.clone(),
        Coin::try_from(10).unwrap(),
        Coin::ZERO,
        1,
        NetworkId::Main,
    );

    let reward = Inherent {
        ty: InherentType::Reward,
        target: address_sender.clone(),
        value: Coin::from_u64_unchecked(10000),
        data: vec![],
    };

    let hash1 = accounts.get_root(None);

    assert_eq!(accounts.get(&KeyNibbles::from(&address_sender), None), None);

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    // Fails as address_sender does not have any funds.
    // Note: When the commit errors, we want to bracket the txn creation and the commit attempt.
    // Otherwise when we try to commit again, the test will get stuck.
    {
        let mut txn = WriteTransaction::new(&env);

        assert!(accounts
            .commit(&mut txn, &[tx.clone()], &[reward.clone()], 1, 1)
            .is_err());
    }

    assert_eq!(accounts.get(&KeyNibbles::from(&address_sender), None), None);

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    assert_eq!(hash1, accounts.get_root(None));

    // Give address_sender one block reward.

    let mut txn = WriteTransaction::new(&env);

    assert!(accounts
        .commit(&mut txn, &[], &[reward.clone()], 1, 1)
        .is_ok());

    txn.commit();

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_sender), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000)
    );

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    let hash2 = accounts.get_root(None);

    assert_ne!(hash1, hash2);

    // Single transaction exceeding funds.
    tx.value = Coin::from_u64_unchecked(1000000);

    {
        let mut txn = WriteTransaction::new(&env);

        assert!(accounts
            .commit(&mut txn, &[tx.clone()], &[reward.clone()], 2, 2)
            .is_err());
    }

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_sender), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000)
    );

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    assert_eq!(hash2, accounts.get_root(None));

    // Multiple transactions exceeding funds.
    tx.value = Coin::from_u64_unchecked(5010);

    let mut tx2 = tx.clone();

    tx2.value += Coin::from_u64_unchecked(10);

    {
        let mut txn = WriteTransaction::new(&env);

        assert!(accounts
            .commit(&mut txn, &vec![tx, tx2], &[reward], 2, 2)
            .is_err());
    }

    assert_eq!(
        accounts
            .get(&KeyNibbles::from(&address_sender), None)
            .unwrap()
            .balance(),
        Coin::from_u64_unchecked(10000)
    );

    assert_eq!(
        accounts.get(&KeyNibbles::from(&address_recipient), None),
        None
    );

    assert_eq!(hash2, accounts.get_root(None));
}

#[test]
fn accounts_performance() {
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
    // Generate and sign transaction from an address
    let num_txns = 10000;
    let mut rng = StdRng::seed_from_u64(0);
    let balance = 1;
    let mut mempool_transactions = vec![];
    let sender_balances = vec![1000000; num_txns];
    let recipient_balances = vec![1000000; num_txns];
    let mut genesis_builder = GenesisBuilder::default();
    let address_validator = Address::from([1u8; Address::SIZE]);
    let reward = Inherent {
        ty: InherentType::Reward,
        target: address_validator.clone(),
        value: Coin::from_u64_unchecked(10000),
        data: vec![],
    };
    let rewards = vec![reward; num_txns];

    // Generate recipient accounts
    let recipient_accounts = generate_accounts(recipient_balances, &mut genesis_builder, false);
    // Generate sender accounts
    let sender_accounts = generate_accounts(sender_balances, &mut genesis_builder, true);

    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: (i + 1) as u64,
            value: balance,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());
    //log::debug!("Done generating {} transactions and accounts", txns.len());

    // Add validator to genesis
    genesis_builder.with_genesis_validator(
        Address::from(&KeyPair::generate(&mut rng.clone())),
        PublicKey::from([0u8; 32]),
        BLSKeyPair::generate(&mut rng).public_key,
        Address::default(),
    );

    let genesis_info = genesis_builder.generate(env.clone()).unwrap();
    let length = genesis_info.accounts.len();
    let accounts = Accounts::new(env.clone());
    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    accounts.init(&mut txn, genesis_info.accounts);
    let duration = start.elapsed();
    println!(
        "Time elapsed after account init: {} ms, Accounts per second {}",
        duration.as_millis(),
        length as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time elapsed after account init's txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );

    println!("Done adding accounts to genesis {}", txns.len());

    // Continuosly generate and send transactions
    let mut height: u32 = 1;
    let mut timestamp: u64 = 1;
    loop {
        println!("Starting new iteration, current block height: {}", height);
        let mut mempool_transactions = vec![];
        // Generate transactions
        for i in 0..num_txns {
            let mempool_transaction = MempoolTransaction {
                fee: 1 as u64,
                value: 1,
                recipient: recipient_accounts[i as usize].clone(),
                sender: sender_accounts[i as usize].clone(),
            };
            mempool_transactions.push(mempool_transaction);
        }

        let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

        let mut txn = WriteTransaction::new(&env);
        let start = Instant::now();
        let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
        match result {
            Ok(_) => assert!(true),
            Err(err) => assert!(false, "Received {}", err),
        };
        let duration = start.elapsed();
        println!(
            "Time elapsed after account commit: {} ms, Accounts per second {}",
            duration.as_millis(),
            num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
        );
        let start = Instant::now();
        txn.commit();
        let duration = start.elapsed();
        println!(
            "Time ellapsed after txn commit: {} ms, Accounts per second {}",
            duration.as_millis(),
            num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
        );
        height += 1;
        timestamp += 1;
    }
    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;

    println!("Starting new iteration, current block height: {}", height);
    let mut mempool_transactions = vec![];
    // Generate transactions
    for i in 0..num_txns {
        let mempool_transaction = MempoolTransaction {
            fee: 1 as u64,
            value: 1,
            recipient: recipient_accounts[i as usize].clone(),
            sender: sender_accounts[i as usize].clone(),
        };
        mempool_transactions.push(mempool_transaction);
    }

    let (txns, _) = generate_transactions(mempool_transactions, rng.clone());

    let mut txn = WriteTransaction::new(&env);
    let start = Instant::now();
    let result = accounts.commit(&mut txn, &txns[..], &rewards[..], height, timestamp);
    match result {
        Ok(_) => assert!(true),
        Err(err) => assert!(false, "Received {}", err),
    };
    let duration = start.elapsed();
    println!(
        "Time elapsed after account commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    let start = Instant::now();
    txn.commit();
    let duration = start.elapsed();
    println!(
        "Time ellapsed after txn commit: {} ms, Accounts per second {}",
        duration.as_millis(),
        num_txns as f64 / (duration.as_millis() as f64 / 1000_f64),
    );
    height += 1;
    timestamp += 1;
}
