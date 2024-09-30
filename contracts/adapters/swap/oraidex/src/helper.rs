use cosmwasm_std::{to_json_binary, Addr, Api, Binary, Deps, StdError};

use oraiswap::asset::AssetInfo;
use oraiswap::converter::ExecuteMsg as ConverterExecuteMsg;
use oraiswap::mixed_router::{
    ExecuteMsg as OraidexRouterExecuteMsg, SwapOperation as OraidexSwapOperation,
};
use oraiswap_v3::{percentage::Percentage, FeeTier, PoolKey};
use skip::swap::SwapOperation;

use crate::{error::ContractError, state::ORAIDEX_ROUTER_ADDRESS};

pub fn denom_to_asset_info(api: &dyn Api, denom: &str) -> AssetInfo {
    if let Ok(contract_addr) = api.addr_validate(denom) {
        AssetInfo::Token { contract_addr }
    } else {
        AssetInfo::NativeToken {
            denom: denom.to_string(),
        }
    }
}

pub fn convert_pool_id_to_v3_pool_key(pool_id: &str) -> Result<PoolKey, ContractError> {
    //poolID:  tokenX-tokenY-fee-tickSpace
    let parts: Vec<&str> = pool_id.split('-').collect();

    if parts.len() != 4 {
        return Err(ContractError::Std(StdError::generic_err(
            "Invalid v3 pool_id, require exactly 4 fields",
        )));
    }

    let token_x = String::from(parts[0]);
    let token_y = String::from(parts[1]);

    let fee = match parts[2].parse::<u64>() {
        Ok(value) => Percentage(value),
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Invalid fee in v3 pool",
            )))
        }
    };
    let tick_spacing = match parts[3].parse::<u16>() {
        Ok(value) => value,
        Err(_) => {
            return Err(ContractError::Std(StdError::generic_err(
                "Invalid tick_spacing in v3 pool",
            )));
        }
    };

    // Create and return the PoolKey instance
    Ok(PoolKey {
        token_x,
        token_y,
        fee_tier: FeeTier { fee, tick_spacing },
    })
}

pub fn parse_to_swap_msg(
    deps: &Deps,
    operation: SwapOperation,
) -> Result<(Addr, Binary), ContractError> {
    // case 1: convert
    if operation.pool.contains("convert") {
        let parts: Vec<&str> = operation.pool.split('-').collect();
        if parts.len() != 2 {
            return Err(ContractError::Std(StdError::generic_err(
                "Invalid convert type pool_id, require exactly 2 fields",
            )));
        }
        let converter = deps.api.addr_validate(parts[1])?;

        match parts[0] {
            "convert_reverse" => {
                return Ok((
                    converter,
                    to_json_binary(&ConverterExecuteMsg::ConvertReverse {
                        from_asset: denom_to_asset_info(deps.api, &operation.denom_in),
                    })?,
                ));
            }
            "convert" => {
                return Ok((converter, to_json_binary(&ConverterExecuteMsg::Convert {})?));
            }
            _ => {
                return Err(ContractError::Std(StdError::generic_err(
                    "Invalid convert type pool_id",
                )));
            }
        }
    }

    let mut hop_swap_requests: Vec<OraidexSwapOperation> = vec![];
    let oraidex_router_contract_address = ORAIDEX_ROUTER_ADDRESS.load(deps.storage)?;

    // case 2: Swap v3
    if operation.pool.contains("-") {
        let pool_key = convert_pool_id_to_v3_pool_key(&operation.pool)?;
        let x_to_y = pool_key.token_x == operation.denom_in;
        hop_swap_requests.push(OraidexSwapOperation::SwapV3 { pool_key, x_to_y });
    } else {
        // case 3: Swap v2
        hop_swap_requests.push(OraidexSwapOperation::OraiSwap {
            offer_asset_info: denom_to_asset_info(deps.api, &operation.denom_in),
            ask_asset_info: denom_to_asset_info(deps.api, &operation.denom_out),
        })
    };

    Ok((
        oraidex_router_contract_address,
        to_json_binary(&OraidexRouterExecuteMsg::ExecuteSwapOperations {
            operations: hop_swap_requests,
            minimum_receive: None,
            to: None,
        })?,
    ))
}
