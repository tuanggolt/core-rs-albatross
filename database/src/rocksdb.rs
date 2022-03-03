use std::fmt;
use std::fs;
use std::sync::Arc;

use ckb_rocksdb::ColumnFamily;
use ckb_rocksdb::DBVector;
use ckb_rocksdb::TransactionDB;
use ckb_rocksdb::ops::OpenCF;
use ckb_rocksdb::ops::GetCF;
use ckb_rocksdb::ops::PutCF;
use ckb_rocksdb::ops::DeleteCF;
use ckb_rocksdb::ops::GetColumnFamilys;
use ckb_rocksdb::ops::Iterate;
use ckb_rocksdb::ops::TransactionBegin;
// re export the lmdb error
pub use lmdb_zero::open;

pub use lmdb_zero::Error as LmdbError;

use super::*;
use crate::cursor::{RawReadCursor, ReadCursor, WriteCursor as WriteCursorTrait};

//#[derive(Debug)]
pub struct RocksDBEnvironment {
    path: String,
    db: Arc<TransactionDB>,
}

impl fmt::Debug for RocksDBEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Debugging Database")
    }
}

impl Clone for RocksDBEnvironment {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            db: Arc::clone(&self.db),
        }
    }
}

impl RocksDBEnvironment {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        path: &str,
        column_families: Vec<&str>,
    ) -> Result<Environment, LmdbError> {
        Ok(Environment::Persistent(
            RocksDBEnvironment::new_rocksdb_environment(path, column_families)?,
        ))
    }

    #[allow(clippy::new_ret_no_self)]
    pub fn new_with_max_readers(
        path: &str,
        column_families: Vec<&str>,
    ) -> Result<Environment, LmdbError> {
        Self::new(path, column_families)
    }

    pub(super) fn new_rocksdb_environment(
        path: &str,
        column_families: Vec<&str>,
    ) -> Result<Self, LmdbError> {
        // fs::create_dir_all(path).unwrap();

        let mut opts = ckb_rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Disable readahead - default is already true, but let's be explicit
        opts.set_advise_random_on_open(true);

        let database = TransactionDB::open_cf(&opts, path, &column_families).unwrap();

        let rocksdb = RocksDBEnvironment {
            path: path.to_string(),
            db: Arc::new(database),
        };

        Ok(rocksdb)
    }

    pub(super) fn open_database(&self, name: String, _flags: DatabaseFlags) -> RocksDatabase {
        let mut opts = ckb_rocksdb::Options::default();
        opts.create_if_missing(true);

        RocksDatabase {
            cf: name.clone(),
            database: Arc::clone(&self.db),
        }
    }

    pub(super) fn drop_database(self) -> io::Result<()> {
        fs::remove_dir_all(self.path())
    }

    fn path(&self) -> String {
        self.path.clone()
    }

    pub fn need_resize(&self, _threshold_size: usize) -> bool {
        false
    }
}

//#[derive(Debug)]
//This is essentially a column family
pub struct RocksDatabase {
    cf: String,
    database: Arc<TransactionDB>,
}

impl fmt::Debug for RocksDatabase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Debugging Database")
    }
}

impl RocksDatabase {
    fn cf_handle(&self) -> &ColumnFamily {
        self.database.cf_handle(&self.cf).expect(&format!("Accessed an unknown column family: {}", self.cf))
    }
}

pub struct RocksDBReadTransaction<'txn> {
    txn: ckb_rocksdb::Transaction<'txn, TransactionDB>,
}

impl<'txn> RocksDBReadTransaction<'txn> {
    pub(super) fn new(env: &'txn RocksDBEnvironment) -> Self {
        let write_options = ckb_rocksdb::WriteOptions::default();

        let mut txn_options = ckb_rocksdb::TransactionOptions::new();
        txn_options.set_snapshot(true);

        let transaction = env.db.transaction(&write_options, &txn_options);

        RocksDBReadTransaction {
            txn: transaction,
        }
    }

    pub(super) fn get<K, V>(&self, db: &RocksDatabase, key: &K) -> Option<V>
    where
        K: AsDatabaseBytes + ?Sized,
        V: FromDatabaseValue,
    {
        let mut read_options = ckb_rocksdb::ReadOptions::default();
        read_options.set_snapshot(&self.txn.snapshot());

        let result: Option<DBVector> = self
            .txn
            .get_cf_opt(db.cf_handle(), AsDatabaseBytes::as_database_bytes(key).as_ref(), &read_options)
            .unwrap();

        Some(FromDatabaseValue::copy_from_database(&result?).unwrap())
    }

    pub(super) fn cursor<'cur>(&self, db: &'cur Database) -> RocksdbCursor<'cur> {
        let cursor = db
            .persistent()
            .unwrap()
            .database
            .raw_iterator();

        RocksdbCursor {
            raw: RawRocksDbCursor { cursor },
        }
    }
}

impl fmt::Debug for RocksDBReadTransaction<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "RocksDBReadTransaction {{}}")
    }
}

pub struct RocksDBWriteTransaction<'txn> {
    txn: ckb_rocksdb::Transaction<'txn, TransactionDB>,
}

impl<'txn> RocksDBWriteTransaction<'txn> {
    pub(super) fn new(env: &'txn RocksDBEnvironment) -> Self {
        let mut txn_options = ckb_rocksdb::TransactionOptions::new();
        txn_options.set_snapshot(true);

        let transaction = env.db.transaction(&ckb_rocksdb::WriteOptions::default(), &txn_options);
        RocksDBWriteTransaction { txn: transaction }
    }

    pub(super) fn get<K, V>(&self, db: &RocksDatabase, key: &K) -> Option<V>
    where
        K: AsDatabaseBytes + ?Sized,
        V: FromDatabaseValue,
    {
        let mut read_options = ckb_rocksdb::ReadOptions::default();
        read_options.set_snapshot(&self.txn.snapshot());

        let result: Option<DBVector> = self
            .txn
            .get_cf_opt(db.cf_handle(), AsDatabaseBytes::as_database_bytes(key).as_ref(), &read_options)
            .unwrap();
        Some(FromDatabaseValue::copy_from_database(&result?).unwrap())
    }

    pub(super) fn put_reserve<K, V>(&mut self, db: &RocksDatabase, key: &K, value: &V)
    where
        K: AsDatabaseBytes + ?Sized,
        V: IntoDatabaseValue + ?Sized,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);
        let value_size = IntoDatabaseValue::database_byte_size(value);

        let mut vec_value = vec![0u8; value_size];
        value.copy_into_database(&mut vec_value);

        self.txn.put_cf(db.cf_handle(), key.as_ref(), vec_value).unwrap();
    }

    pub(super) fn put<K, V>(&mut self, db: &RocksDatabase, key: &K, value: &V)
    where
        K: AsDatabaseBytes + ?Sized,
        V: AsDatabaseBytes + ?Sized,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);
        let value = AsDatabaseBytes::as_database_bytes(value);

        self.txn
            .put_cf(db.cf_handle(), key.as_ref(), value.as_ref())
            .unwrap();
    }

    pub(super) fn remove<K>(&mut self, db: &RocksDatabase, key: &K)
    where
        K: AsDatabaseBytes + ?Sized,
    {
        self.txn
            .delete_cf(db.cf_handle(), AsDatabaseBytes::as_database_bytes(key).as_ref())
            .unwrap();
    }

    pub(super) fn remove_item<K, V>(&mut self, db: &RocksDatabase, key: &K, _value: &V)
    where
        K: AsDatabaseBytes + ?Sized,
        V: AsDatabaseBytes + ?Sized,
    {
        self.txn
            .delete_cf(db.cf_handle(), AsDatabaseBytes::as_database_bytes(key).as_ref())
            .unwrap();
    }

    pub(super) fn commit(self) {
        self.txn.commit().unwrap();
    }

    pub(super) fn cursor<'cur>(&self, db: &'cur Database) -> RocksdbCursor<'cur> {
        let cursor = db
            .persistent()
            .unwrap()
            .database
            .raw_iterator();

        RocksdbCursor {
            raw: RawRocksDbCursor { cursor },
        }
    }

    pub(super) fn write_cursor<'cur>(&self, db: &'cur Database) -> RocksDBWriteCursor<'cur> {
        let cursor = db
            .persistent()
            .unwrap()
            .database
            .raw_iterator();

        RocksDBWriteCursor {
            raw: RawRocksDbCursor { cursor },
        }
    }
}

impl fmt::Debug for RocksDBWriteTransaction<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LmdbWriteTransaction {{}}")
    }
}

pub struct RawRocksDbCursor<'cur> {
    cursor: ckb_rocksdb::DBRawIterator<'cur>,
}

impl RawReadCursor for RawRocksDbCursor<'_> {
    fn first<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        self.cursor.seek_to_first();

        if self.cursor.valid() {
            let key = self.cursor.key().unwrap();
            let value = self.cursor.value().unwrap();

            Some((
                FromDatabaseValue::copy_from_database(key).unwrap(),
                FromDatabaseValue::copy_from_database(value).unwrap(),
            ))
        } else {
            None
        }
    }

    fn first_duplicate<V>(&mut self) -> Option<V>
    where
        V: FromDatabaseValue,
    {
        //Not supported in RockDB
        None
    }

    fn last<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        self.cursor.seek_to_last();

        if self.cursor.valid() {
            let key = self.cursor.key().unwrap();
            let value = self.cursor.value().unwrap();

            Some((
                FromDatabaseValue::copy_from_database(key).unwrap(),
                FromDatabaseValue::copy_from_database(value).unwrap(),
            ))
        } else {
            None
        }
    }

    fn last_duplicate<V>(&mut self) -> Option<V>
    where
        V: FromDatabaseValue,
    {
        //Not supported in RocksDB
        None
    }

    fn seek_key_value<K, V>(&mut self, key: &K, value: &V) -> bool
    where
        K: AsDatabaseBytes + ?Sized,
        V: AsDatabaseBytes + ?Sized,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);
        let _value = AsDatabaseBytes::as_database_bytes(value);

        self.cursor.seek(key);

        if self.cursor.valid() {
            true
        } else {
            false
        }
    }

    fn seek_key_nearest_value<K, V>(&mut self, key: &K, value: &V) -> Option<V>
    where
        K: AsDatabaseBytes + ?Sized,
        V: AsDatabaseBytes + FromDatabaseValue,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);
        let _value = AsDatabaseBytes::as_database_bytes(value);

        self.cursor.seek(key);

        if self.cursor.valid() {
            let value = self.cursor.value().unwrap();
            Some(FromDatabaseValue::copy_from_database(value).unwrap())
        } else {
            None
        }
    }

    fn get_current<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        //Not implemented for rocksdb
        None
    }

    fn next<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        self.cursor.next();

        if self.cursor.valid() {
            let key = self.cursor.key().unwrap();
            let value = self.cursor.value().unwrap();
            Some((
                FromDatabaseValue::copy_from_database(key).unwrap(),
                FromDatabaseValue::copy_from_database(value).unwrap(),
            ))
        } else {
            None
        }
    }

    fn next_duplicate<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        //Not supported in RocksDB
        None
    }

    fn next_no_duplicate<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        //not supported
        None
    }

    fn prev<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        self.cursor.prev();

        if self.cursor.valid() {
            let key = self.cursor.key().unwrap();
            let value = self.cursor.value().unwrap();
            Some((
                FromDatabaseValue::copy_from_database(key).unwrap(),
                FromDatabaseValue::copy_from_database(value).unwrap(),
            ))
        } else {
            None
        }
    }

    fn prev_duplicate<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        //Not supported in RocksDB
        None
    }

    fn prev_no_duplicate<K, V>(&mut self) -> Option<(K, V)>
    where
        K: FromDatabaseValue,
        V: FromDatabaseValue,
    {
        //Not supported in RocksDB
        None
    }

    fn seek_key<K, V>(&mut self, key: &K) -> Option<V>
    where
        K: AsDatabaseBytes + ?Sized,
        V: FromDatabaseValue,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);

        self.cursor.seek(key);

        if self.cursor.valid() {
            let value = self.cursor.value().unwrap();
            Some(FromDatabaseValue::copy_from_database(value).unwrap())
        } else {
            None
        }
    }

    fn seek_key_both<K, V>(&mut self, key: &K) -> Option<(K, V)>
    where
        K: AsDatabaseBytes + FromDatabaseValue,
        V: FromDatabaseValue,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);

        self.cursor.seek(key);

        if self.cursor.valid() {
            let value = self.cursor.value().unwrap();
            let key = self.cursor.key().unwrap();
            Some((
                FromDatabaseValue::copy_from_database(&key).unwrap(),
                FromDatabaseValue::copy_from_database(value).unwrap(),
            ))
        } else {
            None
        }
    }

    fn seek_range_key<K, V>(&mut self, key: &K) -> Option<(K, V)>
    where
        K: AsDatabaseBytes + FromDatabaseValue,
        V: FromDatabaseValue,
    {
        let key = AsDatabaseBytes::as_database_bytes(key);

        self.cursor.seek_for_prev(key);

        if self.cursor.valid() {
            let value = self.cursor.value().unwrap();
            let key = self.cursor.key().unwrap();
            Some((
                FromDatabaseValue::copy_from_database(&key).unwrap(),
                FromDatabaseValue::copy_from_database(value).unwrap(),
            ))
        } else {
            None
        }
    }

    fn count_duplicates(&mut self) -> usize {
        //Not supported in RocksDB
        0
    }
}

pub struct RocksdbCursor<'cur> {
    raw: RawRocksDbCursor<'cur>,
}

impl_read_cursor_from_raw!(RocksdbCursor<'_>, raw);

pub struct RocksDBWriteCursor<'cur> {
    raw: RawRocksDbCursor<'cur>,
}

impl_read_cursor_from_raw!(RocksDBWriteCursor<'_>, raw);

impl WriteCursorTrait for RocksDBWriteCursor<'_> {
    fn remove(&mut self) {
        //Not supported in rokcksdb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_can_save_basic_objects() {
        const DB_NAME: &str = "test";
        let env = RocksDBEnvironment::new("./test", vec![DB_NAME]).unwrap();
        {
            let db = env.open_database(DB_NAME.to_string());

            // Read non-existent value.
            {
                let tx = ReadTransaction::new(&env);
                assert!(tx.get::<str, String>(&db, "test").is_none());
            }

            // Read non-existent value.
            let mut tx = WriteTransaction::new(&env);
            assert!(tx.get::<str, String>(&db, "test").is_none());

            // Write and read value.
            tx.put_reserve(&db, "test", "one");
            assert_eq!(tx.get::<str, String>(&db, "test"), Some("one".to_string()));
            // Overwrite and read value.
            tx.put_reserve(&db, "test", "two");
            assert_eq!(tx.get::<str, String>(&db, "test"), Some("two".to_string()));
            tx.commit();

            // Read value.
            let tx = ReadTransaction::new(&env);
            assert_eq!(tx.get::<str, String>(&db, "test"), Some("two".to_string()));
            tx.close();

            // Remove value.
            let mut tx = WriteTransaction::new(&env);
            tx.remove(&db, "test");
            assert!(tx.get::<str, String>(&db, "test").is_none());
            tx.commit();

            // Check removal.
            {
                let tx = ReadTransaction::new(&env);
                assert!(tx.get::<str, String>(&db, "test").is_none());
            }

            // Write and abort.
            let mut tx = WriteTransaction::new(&env);
            tx.put_reserve(&db, "test", "one");
            tx.abort();

            // Check aborted transaction.
            let tx = ReadTransaction::new(&env);
            assert!(tx.get::<str, String>(&db, "test").is_none());
        }

        env.drop_database().unwrap();
    }

    #[test]
    fn isolation_test() {
        const DB_NAME: &str = "test";
        let env = RocksDBEnvironment::new("./test2", vec![DB_NAME]).unwrap();
        {
            let db = env.open_database(DB_NAME.to_string());

            // Read non-existent value.
            let tx = ReadTransaction::new(&env);
            assert!(tx.get::<str, String>(&db, "test").is_none());

            // WriteTransaction.
            let mut txw = WriteTransaction::new(&env);
            assert!(txw.get::<str, String>(&db, "test").is_none());
            txw.put_reserve(&db, "test", "one");
            assert_eq!(txw.get::<str, String>(&db, "test"), Some("one".to_string()));

            // ReadTransaction should still have the old state.
            assert!(tx.get::<str, String>(&db, "test").is_none());

            // Commit WriteTransaction.
            txw.commit();

            // ReadTransaction should still have the old state.
            assert!(tx.get::<str, String>(&db, "test").is_none());

            // Have a new ReadTransaction read the new state.
            let tx2 = ReadTransaction::new(&env);
            assert_eq!(tx2.get::<str, String>(&db, "test"), Some("one".to_string()));
        }

        env.drop_database().unwrap();
    }

    #[test]
    fn duplicates_test() {
        const DB_NAME: &str = "test";
        let env = RocksDBEnvironment::new("./test3", vec![DB_NAME]).unwrap();
        {
            let db = env.open_database_with_flags(
                DB_NAME.to_string(),
                DatabaseFlags::DUPLICATE_KEYS | DatabaseFlags::DUP_UINT_VALUES,
            );

            // Write one value.
            let mut txw = WriteTransaction::new(&env);
            assert!(txw.get::<str, u32>(&db, "test").is_none());
            txw.put::<str, u32>(&db, "test", &125);
            assert_eq!(txw.get::<str, u32>(&db, "test"), Some(125));
            txw.commit();

            // Have a new ReadTransaction read the new state.
            {
                let tx = ReadTransaction::new(&env);
                assert_eq!(tx.get::<str, u32>(&db, "test"), Some(125));
            }

            // Write a second smaller value.
            let mut txw = WriteTransaction::new(&env);
            assert_eq!(txw.get::<str, u32>(&db, "test"), Some(125));
            txw.put::<str, u32>(&db, "test", &12);
            assert_eq!(txw.get::<str, u32>(&db, "test"), Some(12));
            txw.commit();

            // Have a new ReadTransaction read the smaller value.
            {
                let tx = ReadTransaction::new(&env);
                assert_eq!(tx.get::<str, u32>(&db, "test"), Some(12));
            }

            // Remove smaller value and write larger value.
            let mut txw = WriteTransaction::new(&env);
            assert_eq!(txw.get::<str, u32>(&db, "test"), Some(12));
            txw.remove_item::<str, u32>(&db, "test", &12);
            txw.put::<str, u32>(&db, "test", &5783);
            assert_eq!(txw.get::<str, u32>(&db, "test"), Some(125));
            txw.commit();

            // Have a new ReadTransaction read the smallest value.
            {
                let tx = ReadTransaction::new(&env);
                assert_eq!(tx.get::<str, u32>(&db, "test"), Some(125));
            }

            // Remove everything.
            let mut txw = WriteTransaction::new(&env);
            assert_eq!(txw.get::<str, u32>(&db, "test"), Some(125));
            txw.remove::<str>(&db, "test");
            assert!(txw.get::<str, u32>(&db, "test").is_none());
            txw.commit();

            // Have a new ReadTransaction read the new state.
            {
                let tx = ReadTransaction::new(&env);
                assert!(tx.get::<str, u32>(&db, "test").is_none());
            }
        }

        env.drop_database().unwrap();
    }

    #[test]
    fn cursor_test() {
        const DB_NAME: &str = "test";
        let env = RocksDBEnvironment::new("./test4", vec![DB_NAME]).unwrap();
        {
            let db = env.open_database_with_flags(
                DB_NAME.to_string(),
                DatabaseFlags::DUPLICATE_KEYS | DatabaseFlags::DUP_UINT_VALUES,
            );

            let test1: String = "test1".to_string();
            let test2: String = "test2".to_string();

            // Write some values.
            let mut txw = WriteTransaction::new(&env);
            assert!(txw.get::<str, u32>(&db, "test").is_none());
            txw.put::<str, u32>(&db, "test1", &125);
            txw.put::<str, u32>(&db, "test1", &12);
            txw.put::<str, u32>(&db, "test1", &5783);
            txw.put::<str, u32>(&db, "test2", &5783);
            txw.commit();

            // Have a new ReadTransaction read the new state.
            let tx = ReadTransaction::new(&env);
            let mut cursor = tx.cursor(&db);
            assert_eq!(cursor.first::<String, u32>(), Some((test1.clone(), 12)));
            assert_eq!(cursor.last::<String, u32>(), Some((test2.clone(), 5783)));
            assert_eq!(cursor.prev::<String, u32>(), Some((test1.clone(), 5783)));
            assert_eq!(cursor.first_duplicate::<u32>(), Some(12));
            assert_eq!(
                cursor.next_duplicate::<String, u32>(),
                Some((test1.clone(), 125))
            );
            assert_eq!(
                cursor.prev_duplicate::<String, u32>(),
                Some((test1.clone(), 12))
            );
            assert_eq!(
                cursor.next_no_duplicate::<String, u32>(),
                Some((test2.clone(), 5783))
            );
            assert!(cursor.seek_key::<str, u32>("test").is_none());
            assert_eq!(cursor.seek_key::<str, u32>("test1"), Some(12));
            assert_eq!(cursor.count_duplicates(), 3);
            assert_eq!(cursor.last_duplicate::<u32>(), Some(5783));
            //            assert_eq!(cursor.seek_key_both::<String, u32>(&test1), Some((test1.clone(), 12)));
            assert!(!cursor.seek_key_value::<str, u32>("test1", &15));
            assert!(cursor.seek_key_value::<str, u32>("test1", &125));
            assert_eq!(
                cursor.get_current::<String, u32>(),
                Some((test1.clone(), 125))
            );
            assert_eq!(
                cursor.seek_key_nearest_value::<str, u32>("test1", &126),
                Some(5783)
            );
            assert_eq!(cursor.get_current::<String, u32>(), Some((test1, 5783)));
            assert!(cursor.prev_no_duplicate::<String, u32>().is_none());
            assert_eq!(cursor.next::<String, u32>(), Some((test2, 5783)));
            //            assert_eq!(cursor.seek_range_key::<String, u32>("test"), Some((test1.clone(), 12)));
        }

        env.drop_database().unwrap();
    }
}
