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

// Define the types of the contract's storage.
type TxIdHashType = (SOLAddress, Uint<256>, SOLBytes, SOLBytes, Uint<256>);

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
    // Define the contract's storage.
    #[entrypoint]
    pub struct TimeLock {
        address owner;
        mapping(bytes32 => bool) queued;
    }
}

// Error types for the TimeLock contract
#[derive(SolidityError)]
pub enum TimeLockError {
    // Error for when the sender is not the owner
    NotOwnerError(NotOwnerError),
    // Error for when the transaction is already queued
    AlreadyQueuedError(AlreadyQueuedError),
    // Error for when the timestamp is not in the range
    TimestampNotInRangeError(TimestampNotInRangeError),
    // Error for when the transaction is not queued
    NotQueuedError(NotQueuedError),
    // Error for when the timestamp has not yet passed
    TimestampNotPassedError(TimestampNotPassedError),
    // Error for when the timestamp has expired
    TimestampExpiredError(TimestampExpiredError),
    // Error for when a transaction fails
    TxFailedError(TxFailedError),
}

// Marks `TimeLock` as a contract with the specified external methods
#[external]
impl TimeLock  {

    // Minimum delay allowed for a transaction
    pub const MIN_DELAY: u64 = 10;
    // Maximum delay allowed for a transaction
    pub const MAX_DELAY: u64 = 1000;
    // Grace period after the maximum delay
    pub const GRACE_PERIOD: u64 = 1000;

    // Function to generate a transaction ID
    pub fn get_tx_id(
        &self, 
        target: Address, // Target address for the transaction
        value: U256, // Value to be transferred
        func: String, // Function name to be called
        data: Vec<u8>, // Data to be passed to the function
        timestamp: U256, // Timestamp for the transaction
    ) -> FixedBytes<32>{
        
        // Package the transaction data
        let tx_hash_data = (target, value, func, data, timestamp);
        // Encode the transaction data using ABI encoding
        let tx_hash_bytes = TxIdHashType::abi_encode_sequence(&tx_hash_data);
        // Initialize a new Keccak256 hasher
        let mut hasher = Keccak256::new();
        // Update the hasher with the encoded bytes
        hasher.update(tx_hash_bytes);
        // Finalize the hash computation
        let result = hasher.finalize();
        // Convert the hash result to a vector
        let result_vec = result.to_vec();
        // Create a FixedBytes<32> instance from the result vector
        // This is used as the transaction ID
        alloy_primitives::FixedBytes::<32>::from_slice(&result_vec)
    }

    // Function to queue a transaction for execution
    pub fn queue(
        &mut self,
        target: Address, // Target address for the transaction
        value: U256, // Value to be transferred
        func: String, // Function name to be called
        data: Vec<u8>, // Data to be passed to the function
        timestamp: U256, // Timestamp for the transaction
    ) -> Result<(), TimeLockError> {
        // Check if the caller is the owner of the contract
        if self.owner.get() != msg::sender() {
            // If not, return an error indicating the caller is not the owner
            return Err(TimeLockError::NotOwnerError(NotOwnerError{}));
        };
        
        // Calculate a transaction ID using the provided parameters
        let tx_id = self.get_tx_id(target, value, func.clone(), data.clone(), timestamp);
        // Check if the transaction is already queued
        if self.queued.get(tx_id) {
            return Err(TimeLockError::AlreadyQueuedError(AlreadyQueuedError{txId: tx_id.into()}));
        }

        // Check if the provided timestamp is within the allowed range
        if timestamp < U256::from(block::timestamp()) + U256::from(TimeLock::MIN_DELAY)
            || timestamp > U256::from(block::timestamp()) + U256::from(TimeLock::MAX_DELAY)
        {
            return Err(TimeLockError::TimestampNotInRangeError(TimestampNotInRangeError{blockTimestamp: U256::from(block::timestamp()),timestamp: timestamp}));
        }

        // Set the transaction as queued in the contract's state
        let mut queue_id = self.queued.setter(tx_id);
        queue_id.set(true);
        // If all checks pass and the transaction is successfully queued, return Ok
        Ok(())
    }

    // Function to execute a queued transaction
    pub fn execute(
        &mut self,
        target: Address, // Target address for the transaction
        value: U256, // Value to be transferred
        func: String, // Function name to be called
        data: Vec<u8>, // Data to be passed to the function
        timestamp: U256, // Timestamp for the transaction
    ) -> Result<(), TimeLockError> {
        // Check if the caller is the owner of the contract
        if self.owner.get() != msg::sender() {
            // If not, return an error indicating the caller is not the owner
            return Err(TimeLockError::NotOwnerError(NotOwnerError{}));
        };
        
        // Calculate a transaction ID using the provided parameters
        let tx_id = self.get_tx_id(target, value, func.clone(), data.clone(), timestamp);
        // Check if the transaction is not queued
        if !self.queued.get(tx_id) {
            return Err(TimeLockError::NotQueuedError(NotQueuedError{txId: tx_id.into()}));
        }
        
        // ----|-------------------|-------
        //  timestamp    timestamp + grace period

        // Check if the timestamp has passed
        if U256::from(block::timestamp()) < timestamp {
            return Err(TimeLockError::TimestampNotPassedError(TimestampNotPassedError{blockTimestamp: U256::from(block::timestamp()), timestamp: timestamp}));
        }
        
        // Check if the timestamp has expired
        if U256::from(block::timestamp()) > timestamp + U256::from(TimeLock::GRACE_PERIOD) {
            return Err(TimeLockError::TimestampExpiredError(TimestampExpiredError{blockTimestamp: U256::from(block::timestamp()), expiresAt: timestamp + U256::from(TimeLock::GRACE_PERIOD)}));
        }
        
        // Set the transaction as not queued in the contract's state
        let mut queue_id = self.queued.setter(tx_id);
        queue_id.set(false);
        
        // Call the target contract with the provided parameters
        match call(Call::new().value(value), target, &data) {
            // Log the transaction execution if successful
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
            // Return an error if the transaction fails
            Err(_) => Err(TimeLockError::TxFailedError(TxFailedError{})),
        }
    }

    // Function to cancel a queued transaction
    pub fn cancel(
        &mut self,
        target: Address,
        value: U256,
        func: String,
        data: Vec<u8>,
        timestamp: U256,
    ) -> Result<(), TimeLockError> {
        // Check if the caller is the owner of the contract
        if self.owner.get() != msg::sender() {
            // If not, return an error indicating the caller is not the owner
            return Err(TimeLockError::NotOwnerError(NotOwnerError{}));
        };

        // Calculate a transaction ID using the provided parameters
        let tx_id = self.get_tx_id(target, value, func, data, timestamp);
        // Check if the transaction is not queued
        if !self.queued.get(tx_id) {
            return Err(TimeLockError::NotQueuedError(NotQueuedError{txId: tx_id.into()}));
        }

        // Set the transaction as not queued in the contract's state
        let mut queue_id = self.queued.setter(tx_id);
        queue_id.set(false);

        // Log the transaction cancellation
        evm::log(Cancel {
            txId: tx_id.into(),
        });

        // Return Ok if the transaction is successfully cancelled
        Ok(())
    }

    
}