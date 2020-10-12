use crate::messages::{
    BlockHashes, Epoch, HistoryChunk, RequestBlockHashes, RequestBlockHashesFilter, RequestEpoch,
    RequestHistoryChunk, RequestResponseMessage,
};
use block_albatross::Block;
use blockchain_albatross::{history_store::CHUNK_SIZE, Blockchain, Direction};
use network_interface::message::ResponseMessage;
use nimiq_genesis::NetworkInfo;
use primitives::policy;
use std::sync::Arc;

/// This trait defines the behaviour when receiving a message and how to generate the response.
pub trait Handle<Response> {
    fn handle(&self, blockchain: &Arc<Blockchain>) -> Option<Response>;
}

impl Handle<RequestResponseMessage<BlockHashes>> for RequestResponseMessage<RequestBlockHashes> {
    fn handle(&self, blockchain: &Arc<Blockchain>) -> Option<RequestResponseMessage<BlockHashes>> {
        // A peer has requested blocks. Check all requested block locator hashes
        // in the given order and pick the first hash that is found on our main
        // chain, ignore the rest. If none of the requested hashes is found,
        // pick the genesis block hash. Send the main chain starting from the
        // picked hash back to the peer.
        let network_info = NetworkInfo::from_network_id(blockchain.network_id);
        let mut start_block_hash = network_info.genesis_hash().clone();
        for locator in self.locators.iter() {
            if blockchain
                .chain_store
                .get_block(locator, false, None)
                .is_some()
            {
                // We found a block, ignore remaining block locator hashes.
                start_block_hash = locator.clone();
                break;
            }
        }

        // Collect up to GETBLOCKS_VECTORS_MAX inventory vectors for the blocks starting right
        // after the identified block on the main chain.
        let blocks = match self.filter {
            RequestBlockHashesFilter::ElectionOnly => blockchain
                .get_macro_blocks(
                    &start_block_hash,
                    self.max_blocks as u32,
                    false,
                    Direction::Forward,
                    true,
                )
                .unwrap(), // We made sure that start_block_hash is on our chain.
            RequestBlockHashesFilter::All => blockchain.get_blocks(
                &start_block_hash,
                self.max_blocks as u32,
                false,
                Direction::Forward,
            ),
        };

        let hashes = blocks.iter().map(|block| block.hash()).collect();

        Some(RequestResponseMessage::with_identifier(
            BlockHashes { hashes },
            self.get_request_identifier(),
        ))
    }
}

impl Handle<RequestResponseMessage<Epoch>> for RequestResponseMessage<RequestEpoch> {
    fn handle(&self, blockchain: &Arc<Blockchain>) -> Option<RequestResponseMessage<Epoch>> {
        if let Some(Block::Macro(block)) = blockchain.get_block(&self.hash, true) {
            let epoch = policy::epoch_at(block.header.block_number);
            let history_len = blockchain.get_num_extended_transactions(epoch, None);
            let response = Epoch {
                block,
                history_len: history_len as u64,
            };

            Some(RequestResponseMessage::with_identifier(
                response,
                self.get_request_identifier(),
            ))
        } else {
            None
        }
    }
}

impl Handle<RequestResponseMessage<HistoryChunk>> for RequestResponseMessage<RequestHistoryChunk> {
    fn handle(&self, blockchain: &Arc<Blockchain>) -> Option<RequestResponseMessage<HistoryChunk>> {
        let chunk = blockchain.get_chunk(
            self.epoch_number,
            CHUNK_SIZE,
            self.chunk_index as usize,
            None,
        );
        let response = HistoryChunk { chunk };
        Some(RequestResponseMessage::with_identifier(
            response,
            self.get_request_identifier(),
        ))
    }
}