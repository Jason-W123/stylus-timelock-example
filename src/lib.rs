//!
//! Stylus Cupcake Example
//!
//! The program is ABI-equivalent with Solidity, which means you can call it from both Solidity and Rust.
//! To do this, run `cargo stylus export-abi`.
//!
//! Note: this code is a template-only and has not been audited.
//!

// Allow `cargo stylus export-abi` to generate a main function if the "export-abi" feature is enabled.
#![cfg_attr(not(feature = "export-abi"), no_main)]
extern crate alloc;

// Use an efficient WASM allocator for memory management.
#[global_allocator]
static ALLOC: mini_alloc::MiniAlloc = mini_alloc::MiniAlloc::INIT;

use sha3::{Digest, Keccak256};
use alloc::string::String;
use alloy_primitives::{Address, FixedBytes, U256};
use alloy_sol_types::{sol, sol_data::{Address as SOLAddress, Bytes as SOLBytes, *}, SolType};
// Import items from the SDK. The prelude contains common traits and macros.
use stylus_sdk::{call::{Call, call}, prelude::*, block, msg, evm};


sol!{
    error NotOwnerError();
    error AlreadyQueuedError(bytes32 txId);
    error TimestampNotInRangeError(uint256 blockTimestamp, uint256 timestamp);
    error NotQueuedError(bytes32 txId);
    error TimestampNotPassedError(uint256 blockTimestamp, uint256 timestamp);
    error TimestampExpiredError(uint256 blockTimestamp, uint256 expiresAt);
    error TxFailedError();

    event Queue(
        bytes32 indexed txId,
        address indexed target,
        uint256 value,
        string func,
        bytes data,
        uint256 timestamp
    );
    event Execute(
        bytes32 indexed txId,
        address indexed target,
        uint256 value,
        string func,
        bytes data,
        uint256 timestamp
    );
    event Cancel(bytes32 indexed txId);
}

// Define persistent storage using the Solidity ABI.
// `TimeLock` will be the entrypoint for the contract.
sol_storage! {
    #[entrypoint]
    pub struct TimeLock {
        address owner;
        mapping(bytes32 => bool) queued;
    }
}

#[derive(SolidityError)]
pub enum TimeLockError {
    NotOwnerError(NotOwnerError),
    AlreadyQueuedError(AlreadyQueuedError),
    TimestampNotInRangeError(TimestampNotInRangeError),
    NotQueuedError(NotQueuedError),
    TimestampNotPassedError(TimestampNotPassedError),
    TimestampExpiredError(TimestampExpiredError),
    TxFailedError(TxFailedError),
}

#[external]
impl TimeLock  {

    const MIN_DELAY: u64 = 10;
    const MAX_DELAY: u64 = 1000;
    const GRACE_PERIOD: u64 = 1000;

    pub fn get_tx_id(
        &mut self,
        target: Address,
        value: U256,
        func: String,
        data: Vec<u8>,
        timestamp: U256,
    ) -> FixedBytes<32>{
        type TxIdHashType = (SOLAddress, Uint<256>, SOLBytes, SOLBytes, Uint<256>);
        let tx_hash_data = (target, value, func, data, timestamp);
        let tx_hash_bytes = TxIdHashType::abi_encode_sequence(&tx_hash_data);
        let mut hasher = Keccak256::new();
        hasher.update(tx_hash_bytes);
        let result = hasher.finalize();
        let result_vec = result.to_vec();
        alloy_primitives::FixedBytes::<32>::from_slice(&result_vec)
    }

    pub fn queue(
        &mut self,
        target: Address,
        value: U256,
        func: String,
        data: Vec<u8>,
        timestamp: U256,
    ) -> Result<(), TimeLockError> {
        if self.owner.get() != msg::sender() {
            return Err(TimeLockError::NotOwnerError(NotOwnerError{}));
        };
        
        let tx_id = self.get_tx_id(target, value, func.clone(), data.clone(), timestamp);
        if self.queued.get(tx_id) {
            return Err(TimeLockError::AlreadyQueuedError(AlreadyQueuedError{txId: tx_id.into()}));
        }

        if timestamp < U256::from(block::timestamp()) + U256::from(TimeLock::MIN_DELAY)
            || timestamp > U256::from(block::timestamp()) + U256::from(TimeLock::MAX_DELAY)
        {
            return Err(TimeLockError::TimestampNotInRangeError(TimestampNotInRangeError{blockTimestamp: U256::from(block::timestamp()),timestamp: timestamp}));
        }

        let mut queue_id = self.queued.setter(tx_id);
        queue_id.set(true);

        Ok(())
    }

    pub fn execute(
        &mut self,
        target: Address,
        value: U256,
        func: String,
        data: Vec<u8>,
        timestamp: U256,
    ) -> Result<(), TimeLockError> {
        if self.owner.get() != msg::sender() {
            return Err(TimeLockError::NotOwnerError(NotOwnerError{}));
        };
    
        let tx_id = self.get_tx_id(target, value, func.clone(), data.clone(), timestamp);
        if !self.queued.get(tx_id) {
            return Err(TimeLockError::NotQueuedError(NotQueuedError{txId: tx_id.into()}));
        }
    
        if U256::from(block::timestamp()) < timestamp {
            return Err(TimeLockError::TimestampNotPassedError(TimestampNotPassedError{blockTimestamp: U256::from(block::timestamp()), timestamp: timestamp}));
        }
    
        if U256::from(block::timestamp()) > timestamp + U256::from(TimeLock::GRACE_PERIOD) {
            return Err(TimeLockError::TimestampExpiredError(TimestampExpiredError{blockTimestamp: U256::from(block::timestamp()), expiresAt: timestamp + U256::from(TimeLock::GRACE_PERIOD)}));
        }
        
        let mut queue_id = self.queued.setter(tx_id);
        queue_id.set(false);
    
        match call(Call::new().value(value), target, &data) {
            Ok(_) => {
                evm::log(Execute {
                    txId: tx_id.into(),
                    target,
                    value: value,
                    func: func,
                    data: data.into(),
                    timestamp: timestamp,
                });
                Ok(())
            },
            Err(_) => Err(TimeLockError::TxFailedError(TxFailedError{})),
        }
    }

    pub fn cancel(
        &mut self,
        target: Address,
        value: U256,
        func: String,
        data: Vec<u8>,
        timestamp: U256,
    ) -> Result<(), TimeLockError> {
        if self.owner.get() != msg::sender() {
            return Err(TimeLockError::NotOwnerError(NotOwnerError{}));
        };

        let tx_id = self.get_tx_id(target, value, func, data, timestamp);
        if !self.queued.get(tx_id) {
            return Err(TimeLockError::NotQueuedError(NotQueuedError{txId: tx_id.into()}));
        }

        let mut queue_id = self.queued.setter(tx_id);
        queue_id.set(false);

        evm::log(Cancel {
            txId: tx_id.into(),
        });

        Ok(())
    }

    
}