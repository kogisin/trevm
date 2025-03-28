//! Helpers for system actions including [EIP-4788], [EIP-6110], [EIP-7002] and
//! [EIP-7251].
//!
//! System actions are special state changes or smart contract calls made
//! before or after transaction exection. These actions are introduced via
//! hardfork. System actions are sometimes modeled as transactions with special
//! properties (as in [EIP-4788], [EIP-7002] and [EIP-7251]) or as special state
//! changes outside of the transaction lifecycle (as in [EIP-6110]).
//!
//! System transactions are modeled by the [`SystemTx`] struct, which implements
//! the [`Tx`] trait. The system transactions are sent from a special system
//! caller address: [`DEFAULT_SYSTEM_CALLER`]. Note that the system caller is
//! specified independently in each EIP, which allows introduction off
//! different system callers in future EIPs
//!
//! [`Tx`]: crate::Tx
//!
//! [EIP-4788]: https://eips.ethereum.org/EIPS/eip-4788
//! [EIP-6110]: https://eips.ethereum.org/EIPS/eip-6110
//! [EIP-7002]: https://eips.ethereum.org/EIPS/eip-7002
//! [EIP-7251]: https://eips.ethereum.org/EIPS/eip-7251

mod fill;
pub use fill::{SystemTx, DEFAULT_SYSTEM_CALLER};

/// Helpers for Prague historical block hash [EIP-2935] system actions.
///
/// [EIP-2935]: https://eips.ethereum.org/EIPS/eip-2935
pub mod eip2935;

/// Helpers for Cancun beacon root [EIP-4788] system actions.
///
/// [EIP-4788]: https://eips.ethereum.org/EIPS/eip-4788
pub mod eip4788;

/// Helpers for Cancun withdrawal [EIP-4895] system actions.
///
/// [EIP-4895]: https://eips.ethereum.org/EIPS/eip-4895
pub mod eip4895;

/// Helpers for Shanghai withdrawal [EIP-6110] system actions.
///
/// [EIP-6110]: https://eips.ethereum.org/EIPS/eip-6110
pub mod eip6110;

/// Helpers for Prague withdrawal request [EIP-7002] system actions.
///
/// [EIP-7002]: https://eips.ethereum.org/EIPS/eip-7002
pub mod eip7002;

/// Helpers for Prague consolidation request [EIP-7251] system actions.
///
/// [EIP-7251]: https://eips.ethereum.org/EIPS/eip-7251
pub mod eip7251;

/// The maximum blob gas limit for a block in Cancun.
pub const MAX_BLOB_GAS_PER_BLOCK_CANCUN: u64 = 786_432;

/// The maximum blob gas limit for a block in Prague.
pub const MAX_BLOB_GAS_PER_BLOCK_PRAGUE: u64 = 1_179_648;

use crate::{EvmExtUnchecked, Tx};
use alloy::primitives::{Address, Bytes, U256};
use revm::{
    primitives::{Bytecode, EVMError, ExecutionResult, ResultAndState, KECCAK_EMPTY},
    Database, DatabaseCommit, Evm,
};

fn checked_insert_code<Ext, Db: Database + DatabaseCommit>(
    evm: &mut Evm<'_, Ext, Db>,
    address: Address,
    code: &Bytes,
) -> Result<(), EVMError<Db::Error>> {
    if evm.account(address).map_err(EVMError::Database)?.info.code_hash == KECCAK_EMPTY {
        evm.set_bytecode(address, Bytecode::new_raw(code.clone())).map_err(EVMError::Database)?;
    }
    Ok(())
}

/// Clean up the system call, restoring the block env.
fn cleanup_syscall<Ext, Db>(
    evm: &mut Evm<'_, Ext, Db>,
    result: &mut ResultAndState,
    syscall: &SystemTx,
    old_gas_limit: U256,
    old_base_fee: U256,
) where
    Db: Database + DatabaseCommit,
{
    evm.block_mut().gas_limit = old_gas_limit;
    evm.block_mut().basefee = old_base_fee;

    // Remove the system caller and fees from the state
    let coinbase = evm.block().coinbase;
    let state = &mut result.state;
    state.remove(&syscall.caller);
    state.remove(&coinbase);
}

/// Apply a system transaction as specified in [EIP-4788], [EIP-7002], or
/// [EIP-7251]. This function will execute the system transaction and apply
/// the result if non-error, cleaning up any extraneous state changes, and
/// restoring the block environment.
///
/// [EIP-4788]: https://eips.ethereum.org/EIPS/eip-4788
/// [EIP-7002]: https://eips.ethereum.org/EIPS/eip-7002
/// [EIP-7251]: https://eips.ethereum.org/EIPS/eip-7251
pub(crate) fn execute_system_tx<Ext, Db>(
    evm: &mut Evm<'_, Ext, Db>,
    syscall: &SystemTx,
) -> Result<ExecutionResult, EVMError<Db::Error>>
where
    Db: Database + DatabaseCommit,
{
    let limit = U256::from(evm.tx().gas_limit);
    let old_gas_limit = core::mem::replace(&mut evm.block_mut().gas_limit, limit);
    let old_base_fee = core::mem::replace(&mut evm.block_mut().basefee, U256::ZERO);

    syscall.fill_tx(evm);
    let mut result = evm.transact()?;

    // Cleanup the syscall.
    cleanup_syscall(evm, &mut result, syscall, old_gas_limit, old_base_fee);

    evm.db_mut().commit(result.state);

    // apply result, remove receipt from block outputs.
    Ok(result.result)
}
