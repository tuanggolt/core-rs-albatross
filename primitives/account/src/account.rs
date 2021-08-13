use beserial::{Deserialize, ReadBytesExt, Serialize, SerializingError, WriteBytesExt};
use nimiq_database::WriteTransaction;
use nimiq_keys::Address;
use nimiq_primitives::account::AccountType;
use nimiq_primitives::coin::Coin;
use nimiq_transaction::Transaction;
use nimiq_trie::key_nibbles::KeyNibbles;

use crate::interaction_traits::{AccountInherentInteraction, AccountTransactionInteraction};
use crate::staking_contract::{Staker, Validator};
use crate::{
    AccountError, AccountsTree, BasicAccount, HashedTimeLockedContract, Inherent, StakingContract,
    VestingContract,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-derive", derive(serde::Serialize, serde::Deserialize))]
pub enum Account {
    Basic(BasicAccount),
    Vesting(VestingContract),
    HTLC(HashedTimeLockedContract),
    Staking(StakingContract),
    StakingValidator(Validator),
    StakingValidatorsStaker(Address),
    StakingStaker(Staker),
}

impl Account {
    pub fn account_type(&self) -> AccountType {
        match *self {
            Account::Basic(_) => AccountType::Basic,
            Account::Vesting(_) => AccountType::Vesting,
            Account::HTLC(_) => AccountType::HTLC,
            Account::Staking(_) => AccountType::Staking,
            Account::StakingValidator(_) => AccountType::StakingValidator,
            Account::StakingValidatorsStaker(_) => AccountType::StakingValidatorsStaker,
            Account::StakingStaker(_) => AccountType::StakingStaker,
        }
    }

    pub fn balance(&self) -> Coin {
        match *self {
            Account::Basic(ref account) => account.balance,
            Account::Vesting(ref account) => account.balance,
            Account::HTLC(ref account) => account.balance,
            Account::Staking(ref account) => account.balance,
            Account::StakingValidator(ref account) => account.balance,
            Account::StakingValidatorsStaker(_) => {
                unimplemented!()
            }
            Account::StakingStaker(ref account) => account.active_stake + account.inactive_stake,
        }
    }

    pub fn balance_add(balance: Coin, value: Coin) -> Result<Coin, AccountError> {
        balance
            .checked_add(value)
            .ok_or(AccountError::InvalidCoinValue)
    }

    pub fn balance_sub(balance: Coin, value: Coin) -> Result<Coin, AccountError> {
        match balance.checked_sub(value) {
            Some(result) => Ok(result),
            None => Err(AccountError::InsufficientFunds {
                balance,
                needed: value,
            }),
        }
    }
}

impl AccountTransactionInteraction for Account {
    fn create(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        balance: Coin,
        transaction: &Transaction,
        block_height: u32,
        block_time: u64,
    ) -> Result<(), AccountError> {
        match transaction.recipient_type {
            AccountType::Basic => Err(AccountError::InvalidForRecipient),
            AccountType::Vesting => VestingContract::create(
                accounts_tree,
                db_txn,
                balance,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::HTLC => HashedTimeLockedContract::create(
                accounts_tree,
                db_txn,
                balance,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::Staking => Err(AccountError::InvalidForRecipient),
            _ => {
                unreachable!()
            }
        }
    }

    fn commit_incoming_transaction(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        transaction: &Transaction,
        block_height: u32,
        block_time: u64,
    ) -> Result<Option<Vec<u8>>, AccountError> {
        match transaction.recipient_type {
            AccountType::Basic => BasicAccount::commit_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::Vesting => VestingContract::commit_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::HTLC => HashedTimeLockedContract::commit_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::Staking => StakingContract::commit_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            _ => {
                unreachable!()
            }
        }
    }

    fn revert_incoming_transaction(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        transaction: &Transaction,
        block_height: u32,
        block_time: u64,
        receipt: Option<&Vec<u8>>,
    ) -> Result<(), AccountError> {
        match transaction.recipient_type {
            AccountType::Basic => BasicAccount::revert_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::Vesting => VestingContract::revert_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::HTLC => HashedTimeLockedContract::revert_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::Staking => StakingContract::revert_incoming_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            _ => {
                unreachable!()
            }
        }
    }

    fn commit_outgoing_transaction(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        transaction: &Transaction,
        block_height: u32,
        block_time: u64,
    ) -> Result<Option<Vec<u8>>, AccountError> {
        match transaction.sender_type {
            AccountType::Basic => BasicAccount::commit_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::Vesting => VestingContract::commit_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::HTLC => HashedTimeLockedContract::commit_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            AccountType::Staking => StakingContract::commit_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
            ),
            _ => {
                unreachable!()
            }
        }
    }

    fn revert_outgoing_transaction(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        transaction: &Transaction,
        block_height: u32,
        block_time: u64,
        receipt: Option<&Vec<u8>>,
    ) -> Result<(), AccountError> {
        match transaction.sender_type {
            AccountType::Basic => BasicAccount::revert_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::Vesting => VestingContract::revert_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::HTLC => HashedTimeLockedContract::revert_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::Staking => StakingContract::revert_outgoing_transaction(
                accounts_tree,
                db_txn,
                transaction,
                block_height,
                block_time,
                receipt,
            ),
            _ => {
                unreachable!()
            }
        }
    }
}

impl AccountInherentInteraction for Account {
    fn commit_inherent(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        inherent: &Inherent,
        block_height: u32,
        block_time: u64,
    ) -> Result<Option<Vec<u8>>, AccountError> {
        let key = KeyNibbles::from(&inherent.target);

        let account_type = accounts_tree
            .get(db_txn, &key)
            .ok_or(AccountError::NonExistentAddress {
                address: inherent.target.clone(),
            })?
            .account_type();

        match account_type {
            AccountType::Basic => BasicAccount::commit_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
            ),
            AccountType::Vesting => VestingContract::commit_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
            ),
            AccountType::HTLC => HashedTimeLockedContract::commit_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
            ),
            AccountType::Staking => StakingContract::commit_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
            ),
            _ => {
                unreachable!()
            }
        }
    }

    fn revert_inherent(
        accounts_tree: &AccountsTree,
        db_txn: &mut WriteTransaction,
        inherent: &Inherent,
        block_height: u32,
        block_time: u64,
        receipt: Option<&Vec<u8>>,
    ) -> Result<(), AccountError> {
        let key = KeyNibbles::from(&inherent.target);

        let account_type = accounts_tree
            .get(db_txn, &key)
            .ok_or(AccountError::NonExistentAddress {
                address: inherent.target.clone(),
            })?
            .account_type();

        match account_type {
            AccountType::Basic => BasicAccount::revert_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::Vesting => VestingContract::revert_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::HTLC => HashedTimeLockedContract::revert_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
                receipt,
            ),
            AccountType::Staking => StakingContract::revert_inherent(
                accounts_tree,
                db_txn,
                inherent,
                block_height,
                block_time,
                receipt,
            ),
            _ => {
                unreachable!()
            }
        }
    }
}

impl Serialize for Account {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> Result<usize, SerializingError> {
        let mut size: usize = 0;
        size += Serialize::serialize(&self.account_type(), writer)?;

        match *self {
            Account::Basic(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
            Account::Vesting(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
            Account::HTLC(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
            Account::Staking(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
            Account::StakingValidator(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
            Account::StakingValidatorsStaker(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
            Account::StakingStaker(ref account) => {
                size += Serialize::serialize(&account, writer)?;
            }
        }

        Ok(size)
    }

    fn serialized_size(&self) -> usize {
        let mut size = /*type*/ 1;

        match *self {
            Account::Basic(ref account) => {
                size += Serialize::serialized_size(&account);
            }
            Account::Vesting(ref account) => {
                size += Serialize::serialized_size(&account);
            }
            Account::HTLC(ref account) => {
                size += Serialize::serialized_size(&account);
            }
            Account::Staking(ref account) => {
                size += Serialize::serialized_size(&account);
            }
            Account::StakingValidator(ref account) => {
                size += Serialize::serialized_size(&account);
            }
            Account::StakingValidatorsStaker(ref account) => {
                size += Serialize::serialized_size(&account);
            }
            Account::StakingStaker(ref account) => {
                size += Serialize::serialized_size(&account);
            }
        }

        size
    }
}

impl Deserialize for Account {
    fn deserialize<R: ReadBytesExt>(reader: &mut R) -> Result<Self, SerializingError> {
        let account_type: AccountType = Deserialize::deserialize(reader)?;

        match account_type {
            AccountType::Basic => {
                let account: BasicAccount = Deserialize::deserialize(reader)?;
                Ok(Account::Basic(account))
            }
            AccountType::Vesting => {
                let account: VestingContract = Deserialize::deserialize(reader)?;
                Ok(Account::Vesting(account))
            }
            AccountType::HTLC => {
                let account: HashedTimeLockedContract = Deserialize::deserialize(reader)?;
                Ok(Account::HTLC(account))
            }
            AccountType::Staking => {
                let account: StakingContract = Deserialize::deserialize(reader)?;
                Ok(Account::Staking(account))
            }
            AccountType::StakingValidator => {
                let account: Validator = Deserialize::deserialize(reader)?;
                Ok(Account::StakingValidator(account))
            }
            AccountType::StakingValidatorsStaker => {
                let account: Address = Deserialize::deserialize(reader)?;
                Ok(Account::StakingValidatorsStaker(account))
            }
            AccountType::StakingStaker => {
                let account: Staker = Deserialize::deserialize(reader)?;
                Ok(Account::StakingStaker(account))
            }
        }
    }
}